use crate::error::AppError;
use std::path::{Component, Path, PathBuf};

/// 三平台非法字符的并集（NFR-PLAT-04）：保证 Vault 可跨平台拷贝。
pub const INVALID_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Windows 保留设备名；即使在 Linux 上也拒绝，以保证可迁移性。
pub const RESERVED_NAMES: &[&str] = &[
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    // Windows 同时把上标数字形式视为设备别名（COM¹/COM²/COM³、LPT¹/LPT²/LPT³），
    // 以及控制台设备名 CONIN$/CONOUT$——它们都不是合法文件名（NFR-PLAT-04）
    "COM\u{00b9}",
    "COM\u{00b2}",
    "COM\u{00b3}",
    "LPT\u{00b9}",
    "LPT\u{00b2}",
    "LPT\u{00b3}",
    "CONIN$",
    "CONOUT$",
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
    Ok(())
}

/// 编码穿越检测（AC-SEC-01）：对**整条相对路径**逐层解码（最多 3 层，覆盖双重编码），
/// 若解码结果表达了穿越语义（父目录段 / 根 / 盘符 / NUL）则判定为攻击载荷。
///
/// 两个关键设计：
/// 1. **解码结果绝不用于实际路径解析**——解析始终使用原始字面路径。因此形如
///    a%2Fb.md 的输入会被接受，并被当作**磁盘上真实存在的单段文件名**处理；
/// 2. 判定按**组件**而非子串：report%2E%2Emd 解码为 report..md，只是含连续点号的
///    普通文件名（不是父目录段），必须放行——旧实现按子串判定会误伤真实文件。
fn decoded_traversal(rel_path: &str) -> bool {
    let mut current = rel_path.to_string();
    for _ in 0..3 {
        let Some(decoded) = percent_decode(&current) else {
            return false; // 无可解码内容
        };
        if decoded.contains('\u{0}') {
            return true;
        }
        let normalized = decoded.replace('\\', "/");
        if normalized.starts_with('/') {
            return true;
        }
        let path = Path::new(&normalized);
        if path.is_absolute() {
            return true;
        }
        for comp in path.components() {
            if matches!(
                comp,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ) {
                return true;
            }
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
        // 用 dunce 规范化：Windows 下 std 的 canonicalize 会返回 verbatim 形式
        // （\\?\C:\...），该前缀若进入 VaultInfo.root 与注册表 abs_path，会导致
        // 显示不可读、跨工具复制失效（技术方案 §3.5.2 明确指出 dunce 属 M1 依赖；NFR-PLAT-10）。
        let canonical_root = dunce::canonicalize(root).map_err(|err| {
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
        // 4.5 编码穿越（AC-SEC-01）：判定用解码结果，解析仍用原始字面路径
        if decoded_traversal(rel_path) {
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
        // 与 PathGuard::new 保持一致：dunce 规范化，避免 verbatim 前缀回流
        match dunce::canonicalize(&cur) {
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
    fn canonical_root_has_no_verbatim_prefix() {
        // NFR-PLAT-10 / 技术方案 §3.5.2：进入 VaultInfo.root 与注册表的路径必须是可读形式
        let (_dir, guard) = guard();
        let root = guard.canonical_root().to_string_lossy().to_string();
        assert!(
            !root.starts_with(r"\\?\"),
            "规范化根不得带 verbatim 前缀：{root}"
        );
        let resolved = guard.resolve("a.md").expect("正常路径应放行");
        assert!(
            !resolved.to_string_lossy().starts_with(r"\\?\"),
            "解析结果不得带 verbatim 前缀：{}",
            resolved.display()
        );
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
        let (_d, g) = guard();
        for name in [
            "100%2e5.md",
            "50%off.md",
            "report%2E%2Emd", // 解码为 report..md：仍是单段普通名（旧实现误伤）
            "a%2Fb.md",       // 解码为 a/b.md：按原始字面量解析，磁盘上就是这个名字
        ] {
            validate_segment(name).unwrap_or_else(|err| panic!("{name} 应合法：{err}"));
            g.resolve(name)
                .unwrap_or_else(|err| panic!("{name} 应可解析：{err:?}"));
        }
    }

    #[test]
    fn rejects_encoded_traversal_at_path_level() {
        let (_d, g) = guard();
        for payload in [
            "%2e%2e%2f",
            "%2e%2e%2fetc",
            "..%2f..%2fetc",
            "%2E%2E%5Cwindows",
            "%252e%252e%252f",
            "%2e%2e%2f%2e%2e%2fetc",
        ] {
            assert_eq!(
                g.resolve(payload).unwrap_err().code(),
                "E_PATH_ESCAPE_DENY",
                "{payload} 必须被拒绝"
            );
        }
    }

    #[test]
    fn rejects_win32_reserved_aliases() {
        for name in [
            "COM\u{00b9}.md",
            "COM\u{00b2}",
            "LPT\u{00b3}.txt",
            "CONIN$",
            "CONOUT$.md",
        ] {
            assert!(
                validate_segment(name).is_err(),
                "{name} 是 Windows 设备别名，必须拒绝"
            );
        }
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
