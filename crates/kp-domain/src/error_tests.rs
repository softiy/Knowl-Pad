mod err02_tests {
    use super::*;

    /// ERR-02 回归：由 io::Error 派生的用户可见文案必须是中文，且**不含 io 原文**
    #[test]
    fn io_errors_are_chinese() {
        let cases = [
            std::io::Error::from(std::io::ErrorKind::NotFound),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            std::io::Error::other("disk exploded"),
        ];
        for err in cases {
            let message = AppError::from(err).to_string();
            assert!(
                message
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "用户可见文案必须含中文：{message}"
            );
            assert!(!message.contains("os error"), "不得出现 io 原文：{message}");
            assert!(
                !message.contains("disk exploded"),
                "不得出现 io 原文：{message}"
            );
        }
    }

    #[test]
    fn io_at_uses_relative_path_as_context() {
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let message = AppError::io_at("dir/a.md", &err).to_string();
        assert!(message.contains("dir/a.md"));
        assert!(!message.contains("Permission denied"));
    }
}

#[cfg(test)]
use super::AppError;

#[test]
fn error_codes_are_stable() {
    assert_eq!(AppError::VaultNotOpen.code(), "E_VAULT_NOT_OPEN");
    assert_eq!(
        AppError::VaultPathInvalid("x".into()).code(),
        "E_VAULT_PATH_INVALID"
    );
    assert_eq!(AppError::PathEmpty.code(), "E_PATH_ESCAPE_DENY");
    assert_eq!(AppError::PathAbsolute.code(), "E_PATH_OUTSIDE_VAULT");
    assert_eq!(AppError::PathOutsideVault.code(), "E_PATH_OUTSIDE_VAULT");
    assert_eq!(AppError::PathEscapeDeny.code(), "E_PATH_ESCAPE_DENY");
    assert_eq!(
        AppError::InvalidFilename("x".into()).code(),
        "E_INVALID_FILENAME"
    );
    assert_eq!(
        AppError::FileNotFound("x".into()).code(),
        "E_FILE_NOT_FOUND"
    );
    assert_eq!(AppError::FileExists("x".into()).code(), "E_FILE_EXISTS");
    assert_eq!(
        AppError::WriteConflict("x".into()).code(),
        "E_WRITE_CONFLICT"
    );
    assert_eq!(AppError::IoFailure("x".into()).code(), "E_IO_FAILURE");
    assert_eq!(AppError::DbError("x".into()).code(), "E_DB_ERROR");
}

#[test]
fn io_errors_map_by_kind() {
    let nf = AppError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
    assert_eq!(nf.code(), "E_FILE_NOT_FOUND");
    let denied = AppError::from(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "deny",
    ));
    // PRD §5.2：权限被拒是独立错误码，前端据此提示「检查文件权限」
    assert_eq!(denied.code(), "E_PERMISSION_DENIED");
    let other = AppError::from(std::io::Error::other("disk full"));
    assert_eq!(other.code(), "E_IO_FAILURE");
}
