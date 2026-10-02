//! 命令层统一路径解析入口（SEC-02）。
//!
//! 纪律：**全部接收路径参数的命令必须经此解析**，命令内部不得自行拼接或规范化路径。
//! 前端校验（RT-02）不作为安全边界——Rust 侧始终独立执行七步校验。

use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::path_guard::PathGuard;
use std::path::{Path, PathBuf};

/// 取当前 Vault 根；未打开时返回 E_VAULT_NOT_OPEN。
pub(crate) fn root_of(state: &AppState) -> Result<PathBuf, KpError> {
    state.current_root().ok_or(KpError(AppError::VaultNotOpen))
}

/// 在给定 Vault 根下执行七步校验并返回安全的绝对路径。
///
/// 校验失败时**记录日志**（SEC-02 明确要求）：只记错误码与相对路径，
/// **不记文件内容**（红线 R-12）。
pub(crate) fn resolve_in(root: &Path, rel_path: &str) -> Result<PathBuf, KpError> {
    let guard = PathGuard::new(root).map_err(KpError)?;
    match guard.resolve(rel_path) {
        Ok(path) => Ok(path),
        Err(err) => {
            tracing::warn!(
                code = err.code(),
                rel_path = rel_path,
                "路径校验拒绝（SEC-02）"
            );
            Err(KpError(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_in_rejects_traversal_and_accepts_normal() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        fs::create_dir_all(dir.path().join("notes")).expect("应可建目录");
        let ok = resolve_in(dir.path(), "notes/a.md").expect("正常路径应通过");
        // 基准取 PathGuard 的规范化根（与实现同源）：Windows 下 std::fs::canonicalize
        // 会带 \\?\ 前缀，用它作基准会与 dunce 规范化后的结果不一致
        let root = kp_domain::path_guard::PathGuard::new(dir.path())
            .expect("根应可建立校验器")
            .canonical_root()
            .to_path_buf();
        assert!(
            ok.starts_with(&root),
            "{} 应位于 {} 内",
            ok.display(),
            root.display()
        );
        assert!(
            !ok.to_string_lossy().starts_with(r"\\?\"),
            "解析结果不得带 verbatim 前缀"
        );
        let err = resolve_in(dir.path(), "../escape.md").expect_err("穿越必须被拒绝");
        assert_eq!(err.0.code(), "E_PATH_OUTSIDE_VAULT");
    }

    #[test]
    fn resolve_in_reports_encoded_payload() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let err = resolve_in(dir.path(), "%2e%2e%2fetc").expect_err("编码穿越必须被拒绝");
        assert_eq!(err.0.code(), "E_PATH_ESCAPE_DENY");
    }
    /// AC-SEC-01 载荷矩阵（与 kp-domain 的穿越测试集保持同一口径）。
    fn traversal_payloads() -> Vec<(&'static str, String)> {
        vec![
            ("经典穿越", "../../../etc/passwd".to_string()),
            (
                "反斜杠穿越",
                "..\\..\\windows\\system32\\config".to_string(),
            ),
            ("相对穿越", "folder/../../outside.md".to_string()),
            ("编码穿越", "%2e%2e%2fetc".to_string()),
            ("双重编码", "%252e%252e%252fetc".to_string()),
            ("NUL 字节", "a\u{0}b.md".to_string()),
            ("绝对路径", "/etc/passwd".to_string()),
            ("盘符", "C:/Windows/x.md".to_string()),
            ("仅父目录", "..".to_string()),
        ]
    }

    const DENY_CODES: [&str; 2] = ["E_PATH_OUTSIDE_VAULT", "E_PATH_ESCAPE_DENY"];

    #[test]
    fn traversal_payloads_cannot_write_outside_vault() {
        // #6 修复：此前的「无副作用」用例只调用纯只读的 resolve()，结构上不可能失败。
        // 本用例**驱动命令层真实写入路径**（resolve_in → note_io::write_note），
        // 因此若任一载荷被错误放行，Vault 外的哨兵文件/目录就会被改动，断言随即失败。
        let vault = tempfile::tempdir().expect("Vault 临时目录应可创建");
        let outside = tempfile::tempdir().expect("外部临时目录应可创建");
        let sentinel = outside.path().join("sentinel.txt");
        fs::write(&sentinel, b"PRECIOUS").expect("哨兵应可写");
        let before = list_dir(outside.path());

        for (label, payload) in traversal_payloads() {
            match resolve_in(vault.path(), &payload) {
                Ok(path) => {
                    // 放行也不能越界：解析结果必须仍在 Vault 内
                    assert!(
                        path.starts_with(vault.path().canonicalize().expect("根应可规范化")),
                        "{label} 解析到 Vault 外：{}",
                        path.display()
                    );
                    // 真实走一次写入，验证「放行即无副作用」这一断言确实有内容
                    let _ = kp_domain::note_io::write_note(&path, b"written", None);
                }
                Err(err) => assert!(
                    DENY_CODES.contains(&err.0.code()),
                    "{label}（{payload:?}）应返回穿越类错误码，实得 {}",
                    err.0.code()
                ),
            }
        }

        // 关键断言：Vault 外零副作用
        assert_eq!(
            fs::read(&sentinel).expect("哨兵应仍可读"),
            b"PRECIOUS",
            "Vault 外文件被改动"
        );
        assert_eq!(list_dir(outside.path()), before, "Vault 外目录条目发生变化");
    }

    #[cfg(windows)]
    #[test]
    fn junction_escape_is_blocked_on_windows() {
        // #6 补齐：Windows 上等价于符号链接、且**无需管理员权限**的攻击面是目录 junction
        let vault = tempfile::tempdir().expect("Vault 临时目录应可创建");
        let outside = tempfile::tempdir().expect("外部临时目录应可创建");
        let link = vault.path().join("junction");
        let created = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output();
        let ok = matches!(&created, Ok(out) if out.status.success());
        if !ok {
            eprintln!("跳过：无法创建目录 junction（非 NTFS 或权限不足）");
            return;
        }
        let err =
            resolve_in(vault.path(), "junction/evil.md").expect_err("junction 逃逸必须被拒绝");
        assert!(DENY_CODES.contains(&err.0.code()), "实得 {}", err.0.code());
    }

    /// 目录条目快照（排序后比较）。
    fn list_dir(path: &Path) -> Vec<String> {
        let mut entries: Vec<String> = fs::read_dir(path)
            .expect("目录应可读")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();
        entries
    }
}
