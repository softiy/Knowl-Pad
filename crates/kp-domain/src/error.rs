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
    /// FR-STORAGE-02：第三方软件配置目录（.obsidian/.git）与应用内部目录（.knowlpad）**不得被修改**
    #[error("这是第三方软件或应用内部目录（{0}），Knowl Pad 不会修改它的内容")]
    ProtectedDir(String),
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
    /// PRD §5.2 / §5.3.3：改写预览已过期或已被消费（一次性使用）。
    #[error("改写预览已过期或已被消费，请重新执行预览")]
    PreviewExpired,
    /// PRD §5.2：批量链接改写失败（**已回滚**）——消息里带失败文件清单。
    #[error("批量链接改写失败（已回滚）：{0}")]
    RewriteFailed(String),
}

impl AppError {
    /// 映射为 PRD §5.2 的错误码（前端契约）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::VaultNotOpen => "E_VAULT_NOT_OPEN",
            Self::VaultPathInvalid(_) => "E_VAULT_PATH_INVALID",
            Self::PathEmpty | Self::PathEscapeDeny => "E_PATH_ESCAPE_DENY",
            Self::PathAbsolute | Self::PathOutsideVault => "E_PATH_OUTSIDE_VAULT",
            // 复用既有错误码，避免改动 PRD §5.2 的契约表
            Self::ProtectedDir(_) => "E_PATH_ESCAPE_DENY",
            Self::InvalidFilename(_) => "E_INVALID_FILENAME",
            Self::FileNotFound(_) => "E_FILE_NOT_FOUND",
            Self::FileExists(_) => "E_FILE_EXISTS",
            Self::WriteConflict(_) => "E_WRITE_CONFLICT",
            Self::FileLocked(_) => "E_FILE_LOCKED",
            Self::PermissionDenied(_) => "E_PERMISSION_DENIED",
            Self::IoFailure(_) => "E_IO_FAILURE",
            Self::DbError(_) => "E_DB_ERROR",
            Self::PreviewExpired => "E_PREVIEW_EXPIRED",
            Self::RewriteFailed(_) => "E_REWRITE_FAILED",
        }
    }
}

impl AppError {
    /// 存储层失败的统一构造（PRD ERR-02）。
    ///
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
    pub fn kind(&self) -> &'static str {
        match self {
            Self::VaultNotOpen => "VaultNotOpen",
            Self::VaultPathInvalid(_) => "VaultPathInvalid",
            Self::PathEmpty => "PathEmpty",
            Self::PathAbsolute => "PathAbsolute",
            Self::PathEscapeDeny => "PathEscapeDeny",
            Self::PathOutsideVault => "PathOutsideVault",
            Self::ProtectedDir(_) => "ProtectedDir",
            Self::InvalidFilename(_) => "InvalidFilename",
            Self::FileNotFound(_) => "FileNotFound",
            Self::FileExists(_) => "FileExists",
            Self::WriteConflict(_) => "WriteConflict",
            Self::FileLocked(_) => "FileLocked",
            Self::PermissionDenied(_) => "PermissionDenied",
            Self::IoFailure(_) => "IoFailure",
            Self::DbError(_) => "DbError",
            Self::PreviewExpired => "PreviewExpired",
            Self::RewriteFailed(_) => "RewriteFailed",
        }
    }
}

impl From<std::io::Error> for AppError {
    /// ERR-02：用户可见文案必须**中文、可操作**——**绝不把 io 原文（英文 + os error 码）放进 message**。
    ///
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
#[path = "error_tests.rs"]
mod tests;
