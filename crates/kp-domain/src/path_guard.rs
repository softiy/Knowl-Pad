use crate::error::AppError;
use std::path::{Component, Path, PathBuf};

/// 三平台非法字符的并集（NFR-PLAT-04）：保证 Vault 可跨平台拷贝。
pub const INVALID_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Windows 保留设备名；即使在 Linux 上也拒绝，以保证可迁移性。
pub const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 单段文件名校验（FR-FILE-12）。
pub fn validate_segment(seg: &str) -> Result<(), AppError> {
    if seg.is_empty() || seg.trim_matches('.').is_empty() {
        return Err(AppError::InvalidFilename(seg.to_string()));
    }
    if seg.chars().count() > 200 {
        return Err(AppError::InvalidFilename(
            "文件名过长（上限 200 字符）".into(),
        ));
    }
    if seg.chars().any(char::is_control) {
        return Err(AppError::InvalidFilename("文件名含控制字符".into()));
    }
    if let Some(c) = seg.chars().find(|c| INVALID_CHARS.contains(c)) {
        return Err(AppError::InvalidFilename(format!(
            "文件名不得包含字符 '{c}'"
        )));
    }
    if seg.starts_with(' ') || seg.ends_with(' ') || seg.ends_with('.') {
        return Err(AppError::InvalidFilename(
            "文件名不得以空格开头或以空格/点号结尾".into(),
        ));
    }
    let stem = seg.split('.').next().unwrap_or(seg);
    if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(AppError::InvalidFilename(format!(
            "'{stem}' 是 Windows 保留设备名"
        )));
    }
    // 百分号编码的穿越载荷（AC-SEC-01 明确要求拒绝 %2e%2e%2f 一类输入）
    if contains_encoded_traversal(seg) {
        return Err(AppError::PathEscapeDeny);
    }
    Ok(())
}

/// 百分号编码穿越检测：先解码再判断是否含路径语义（最多解码 3 层，覆盖双重编码）。
///
/// 设计取舍：**不**一刀切拒绝所有百分号——普通文件名允许含 %（例如 100%2e5.md 解码后
/// 只是 100.5.md，不含任何路径语义）。只有解码结果出现 .. / 分隔符 / NUL 时才判定为攻击载荷。
fn contains_encoded_traversal(seg: &str) -> bool {
    let mut current = seg.to_string();
    for _ in 0..3 {
        let Some(decoded) = percent_decode(&current) else {
            return false; // 无可解码内容
        };
        if decoded.contains("..")
            || decoded.contains('/')
            || decoded.contains('\\')
            || decoded.contains('\u{0}')
        {
            return true;
        }
        if decoded == current {
            return false;
        }
        current = decoded;
    }
    false
}

/// 解码百分号转义；无 % 或无可解码内容时返回 None。
fn percent_decode(input: &str) -> Option<String> {
    if !input.contains('%') {
        return None;
    }
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    let mut decoded_any = false;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                index += 3;
                decoded_any = true;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    if !decoded_any {
        return None;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Vault 根限定的路径校验器（SEC-02 / AC-SEC-01 的七步校验）。
#[derive(Debug, Clone)]
pub struct PathGuard {
    canonical_root: PathBuf,
}

impl PathGuard {
    pub fn new(root: &Path) -> Result<Self, AppError> {
        let canonical_root = std::fs::canonicalize(root).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::PathOutsideVault
            } else {
                AppError::from(err)
            }
        })?;
        Ok(Self { canonical_root })
    }

    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    /// 校验相对路径并返回安全的绝对路径。全部失败路径都必须返回 Err，绝不"尽力而为"。
    pub fn resolve(&self, rel_path: &str) -> Result<PathBuf, AppError> {
        // 1 空路径
        if rel_path.is_empty() {
            return Err(AppError::PathEmpty);
        }
        // 2 绝对路径 / 盘符
        if rel_path.starts_with('/') || rel_path.contains(':') {
            return Err(AppError::PathAbsolute);
        }
        // 3 Windows 风格分隔符统一按分隔符处理：保证**三平台判定一致**
        // （\ 本就在 INVALID_CHARS 中，合法文件名不可能含它，故不存在误伤）
        let normalized = rel_path.replace('\\', "/");
        let raw = Path::new(&normalized);
        if raw.is_absolute() {
            return Err(AppError::PathAbsolute);
        }
        // 4 NUL 字节
        if rel_path.bytes().any(|byte| byte == 0) {
            return Err(AppError::PathEscapeDeny);
        }
        // 5 逐段检查
        for comp in raw.components() {
            match comp {
                Component::ParentDir => return Err(AppError::PathOutsideVault),
                Component::RootDir | Component::Prefix(_) => return Err(AppError::PathAbsolute),
                Component::CurDir => return Err(AppError::PathEscapeDeny),
                Component::Normal(seg) => validate_segment(&seg.to_string_lossy())?,
            }
        }
        // 6-7 词法规范化 + 组件级前缀校验
        let joined = self.canonical_root.join(raw);
        let lexical = lexical_normalize(&joined);
        if !lexical.starts_with(&self.canonical_root) {
            return Err(AppError::PathOutsideVault);
        }
        // 8 解析符号链接后再次校验（防逃逸，T-04）
        let canonical = canonicalize_existing(&lexical)?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(AppError::PathEscapeDeny);
        }
        Ok(canonical)
    }
}

/// 词法规范化：只保留 Normal 段，消除任何点段（防御性；正常情况下上游已拒绝）。
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 目标可能尚不存在（新建文件）：向上找到最近的已存在祖先做 canonicalize，再拼回尾巴。
fn canonicalize_existing(path: &Path) -> Result<PathBuf, AppError> {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        match std::fs::canonicalize(&cur) {
            Ok(found) => {
                let mut full = found;
                for seg in tail.iter().rev() {
                    full.push(seg);
                }
                return Ok(full);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let (Some(name), Some(parent)) = (cur.file_name(), cur.parent()) else {
                    return Err(AppError::from(err));
                };
                tail.push(name.to_os_string());
                cur = parent.to_path_buf();
            }
            Err(err) => return Err(AppError::from(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn guard() -> (tempfile::TempDir, PathGuard) {
        let dir = tempfile::tempdir().unwrap();
        let guard = PathGuard::new(dir.path()).unwrap();
        (dir, guard)
    }

    #[test]
    fn accepts_plain_and_nested_paths() {
        let (dir, guard) = guard();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        let p = guard.resolve("a/b/note.md").unwrap();
        assert!(p.ends_with("note.md"));
        assert!(p.starts_with(guard.canonical_root()));
    }

    #[test]
    fn rejects_empty() {
        let (_d, g) = guard();
        assert_eq!(g.resolve("").unwrap_err().code(), "E_PATH_ESCAPE_DENY");
    }

    #[test]
    fn rejects_absolute_and_drive() {
        let (_d, g) = guard();
        assert_eq!(
            g.resolve("/etc/passwd").unwrap_err().code(),
            "E_PATH_OUTSIDE_VAULT"
        );
        assert_eq!(
            g.resolve("C:/Windows/x").unwrap_err().code(),
            "E_PATH_OUTSIDE_VAULT"
        );
    }

    #[test]
    fn rejects_parent_and_curdir() {
        let (_d, g) = guard();
        assert_eq!(
            g.resolve("../x").unwrap_err().code(),
            "E_PATH_OUTSIDE_VAULT"
        );
        assert_eq!(
            g.resolve("a/../../x").unwrap_err().code(),
            "E_PATH_OUTSIDE_VAULT"
        );
        assert_eq!(g.resolve("./x").unwrap_err().code(), "E_PATH_ESCAPE_DENY");
    }

    #[test]
    fn rejects_nul_byte() {
        let (_d, g) = guard();
        let evil = String::from_iter(['a', '\u{0}', 'b']);
        assert_eq!(g.resolve(&evil).unwrap_err().code(), "E_PATH_ESCAPE_DENY");
    }

    #[test]
    fn rejects_invalid_filenames() {
        let (_d, g) = guard();
        let bad = [
            "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "name.", " name", "CON", "com1.md", "..",
            "...",
        ];
        for item in bad {
            assert!(g.resolve(item).is_err(), "{item} should be rejected");
        }
    }

    #[test]
    fn allows_dotted_and_reserved_like_names() {
        validate_segment("note.md").unwrap();
        validate_segment("a.b.c").unwrap();
        validate_segment("console").unwrap();
        validate_segment(".gitignore").unwrap();
    }

    #[test]
    fn rejects_long_and_control() {
        assert!(validate_segment(&"x".repeat(201)).is_err());
        let ctrl = String::from_iter(['a', '\u{7}', 'b']);
        assert!(validate_segment(&ctrl).is_err());
    }

    #[test]
    fn rejects_colon_segment() {
        let (_d, g) = guard();
        assert!(g.resolve("a:b").is_err());
    }

    #[test]
    fn resolves_nonexistent_nested_file() {
        let (_d, g) = guard();
        let p = g.resolve("new/dir/file.md").unwrap();
        assert!(p.to_string_lossy().ends_with("file.md"));
    }

    #[test]
    fn lexical_normalize_removes_dots() {
        let p = lexical_normalize(Path::new("a/./b/../c"));
        assert_eq!(p.to_string_lossy().replace('\\', "/"), "a/c");
    }

    #[test]
    fn guard_errors_when_root_missing() {
        let missing = std::env::temp_dir().join("kp-definitely-missing-root-xyz");
        assert!(PathGuard::new(&missing).is_err());
    }

    #[test]
    fn windows_separators_are_treated_consistently() {
        // 同一载荷在三平台必须得到**相同判定**：反斜杠按分隔符解析
        let (_d, g) = guard();
        assert_eq!(
            g.resolve("..\\..\\windows\\system32\\config")
                .unwrap_err()
                .code(),
            "E_PATH_OUTSIDE_VAULT"
        );
        // 合法嵌套路径用反斜杠书写时，等价于正斜杠写法
        let a = g.resolve("notes\\a.md").expect("反斜杠嵌套路径应放行");
        let b = g.resolve("notes/a.md").expect("正斜杠嵌套路径应放行");
        assert_eq!(a, b, "两种分隔符必须解析到同一路径");
    }

    #[test]
    fn rejects_percent_encoded_traversal() {
        let (_d, g) = guard();
        // AC-SEC-01 指定的载荷
        for payload in [
            "%2e%2e%2f",
            "%2e%2e%2Fetc",
            "%2E%2E%5Cwindows",
            "%252e%252e%252f",
        ] {
            assert_eq!(
                g.resolve(payload).unwrap_err().code(),
                "E_PATH_ESCAPE_DENY",
                "{payload} 必须被拒绝"
            );
        }
    }

    #[test]
    fn allows_benign_percent_names() {
        // 普通文件名中的百分号不构成穿越，仍应放行
        validate_segment("100%2e5.md").expect("非穿越语义的编码名应允许");
        validate_segment("50%off.md").expect("百分号本身合法");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let outside = tempfile::tempdir().unwrap();
        let (dir, guard) = guard();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert_eq!(
            guard.resolve("link/evil.md").unwrap_err().code(),
            "E_PATH_ESCAPE_DENY"
        );
    }
}
