use thiserror::Error;

/// 统一错误类型：全部对外错误收敛于此，并映射为 PRD §5.2 的错误码（RS-04）。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("当前没有打开的知识库")]
    VaultNotOpen,
    #[error("知识库路径不存在或不可访问：{0}")]
    VaultPathInvalid(String),
    #[error("路径为空")]
    PathEmpty,
    #[error("不接受绝对路径")]
    PathAbsolute,
    #[error("路径含非法字符")]
    PathEscapeDeny,
    #[error("路径越界")]
    PathOutsideVault,
    #[error("文件名不合法：{0}")]
    InvalidFilename(String),
    #[error("文件不存在：{0}")]
    FileNotFound(String),
    #[error("目标已存在：{0}")]
    FileExists(String),
    #[error("文件已被外部修改，请选择处理方式：{0}")]
    WriteConflict(String),
    #[error("文件被外部程序占用：{0}")]
    FileLocked(String),
    #[error("读写失败：{0}")]
    IoFailure(String),
    #[error("操作系统拒绝访问：{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    DbError(String),
}

impl AppError {
    /// 映射为 PRD §5.2 的错误码（前端契约）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::VaultNotOpen => "E_VAULT_NOT_OPEN",
            Self::VaultPathInvalid(_) => "E_VAULT_PATH_INVALID",
            Self::PathEmpty | Self::PathEscapeDeny => "E_PATH_ESCAPE_DENY",
            Self::PathAbsolute | Self::PathOutsideVault => "E_PATH_OUTSIDE_VAULT",
            Self::InvalidFilename(_) => "E_INVALID_FILENAME",
            Self::FileNotFound(_) => "E_FILE_NOT_FOUND",
            Self::FileExists(_) => "E_FILE_EXISTS",
            Self::WriteConflict(_) => "E_WRITE_CONFLICT",
            Self::FileLocked(_) => "E_FILE_LOCKED",
            Self::PermissionDenied(_) => "E_PERMISSION_DENIED",
            Self::IoFailure(_) => "E_IO_FAILURE",
            Self::DbError(_) => "E_DB_ERROR",
        }
    }
}

impl AppError {
    /// 存储层失败的统一构造（PRD ERR-02）。
    ///
    /// 用户可见 message 必须是**中文、非技术性、含可操作建议**；SQL 原文等技术细节
    /// 由调用方写日志（kc-domain 不依赖日志框架，故日志在存储层完成），**绝不进 message**。
    pub fn db(context: &str) -> Self {
        Self::DbError(format!(
            "{context}失败。请重试；若持续出现，请检查应用数据目录是否可写，或重启应用。"
        ))
    }

    /// 带「路径上下文」的 io 错误构造（ERR-02）：message 里出现的是**相对路径**，不是 io 原文。
    pub fn io_at(rel_path: &str, err: &std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::FileNotFound(rel_path.to_string()),
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied(rel_path.to_string()),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::ResourceBusy => {
                Self::FileLocked(rel_path.to_string())
            }
            _ => Self::IoFailure(rel_path.to_string()),
        }
    }

    /// 变体名（**不含用户数据**）。用于日志的「上下文」字段——SEC-09 禁止在日志中
    /// 记录笔记正文与 Vault 绝对路径，故此处只暴露错误**类别**。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::VaultNotOpen => "VaultNotOpen",
            Self::VaultPathInvalid(_) => "VaultPathInvalid",
            Self::PathEmpty => "PathEmpty",
            Self::PathAbsolute => "PathAbsolute",
            Self::PathEscapeDeny => "PathEscapeDeny",
            Self::PathOutsideVault => "PathOutsideVault",
            Self::InvalidFilename(_) => "InvalidFilename",
            Self::FileNotFound(_) => "FileNotFound",
            Self::FileExists(_) => "FileExists",
            Self::WriteConflict(_) => "WriteConflict",
            Self::FileLocked(_) => "FileLocked",
            Self::PermissionDenied(_) => "PermissionDenied",
            Self::IoFailure(_) => "IoFailure",
            Self::DbError(_) => "DbError",
        }
    }
}

impl From<std::io::Error> for AppError {
    /// ERR-02：用户可见文案必须**中文、可操作**——**绝不把 io 原文（英文 + os error 码）放进 message**。
    ///
    /// 这些变体的载荷会直接插进 message，因此这里放的是「给用户的处置建议」；
    /// 原始 io 细节由调用方写日志（ERR-04），需要路径上下文时用 [`AppError::io_at`]。
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => {
                Self::FileNotFound("文件可能已被移动或删除，请刷新后重试".to_string())
            }
            // PRD §5.2 有独立错误码：前端需要区分「没权限」与「磁盘/读写失败」
            std::io::ErrorKind::PermissionDenied => {
                Self::PermissionDenied("请检查文件或目录权限后重试".to_string())
            }
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::ResourceBusy => {
                Self::FileLocked("请关闭占用该文件的程序后重试".to_string())
            }
            _ => Self::IoFailure("请确认磁盘空间充足、文件未被占用，然后重试".to_string()),
        }
    }
}

#[cfg(test)]
mod err02_tests {
    use super::*;

    /// ERR-02 回归：由 io::Error 派生的用户可见文案必须是中文，且**不含 io 原文**
    /// （曾出现「文件不存在：No such file or directory (os error 2)」）。
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

    /// 带路径上下文时，message 里出现的是**相对路径**，仍然不出现 io 原文。
    #[test]
    fn io_at_uses_relative_path_as_context() {
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let message = AppError::io_at("dir/a.md", &err).to_string();
        assert!(message.contains("dir/a.md"));
        assert!(!message.contains("Permission denied"));
    }
}

#[cfg(test)]
mod tests {
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
}
