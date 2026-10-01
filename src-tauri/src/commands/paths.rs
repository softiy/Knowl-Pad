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
        assert!(ok.starts_with(fs::canonicalize(dir.path()).expect("根应可规范化")));
        let err = resolve_in(dir.path(), "../escape.md").expect_err("穿越必须被拒绝");
        assert_eq!(err.0.code(), "E_PATH_OUTSIDE_VAULT");
    }

    #[test]
    fn resolve_in_reports_encoded_payload() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let err = resolve_in(dir.path(), "%2e%2e%2fetc").expect_err("编码穿越必须被拒绝");
        assert_eq!(err.0.code(), "E_PATH_ESCAPE_DENY");
    }
}
