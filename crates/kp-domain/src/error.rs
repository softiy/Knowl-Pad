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
    /// 与 `DbError` **同码同 kind**，但多带一条**只进日志、不面向用户**的细节（ERR-02 + ERR-04）。
    ///
    /// 审计 R5/G3：全仓 ~112 处 `map_err(|_| AppError::db("…"))` 把 SQLite 的英文原文直接丢掉，
    /// 事后只剩"统计索引实体失败"，无法定位是锁冲突、语法错还是磁盘满。
    /// 面向用户的 message 必须保持中文、可操作（ERR-02），所以细节走**独立字段**：
    /// 日志出口（src-tauri/src/error_wrapper.rs）会以 ERROR 级记录它。
    #[error("{message}")]
    DbErrorDetail {
        /// 面向用户的中文文案（与 `AppError::db` 生成的完全一致）
        message: String,
        /// 仅用于日志的底层细节
        detail: String,
    },
    /// PRD §5.2 / §5.3.3：改写预览已过期或已被消费（一次性使用）。
    #[error("改写预览已过期或已被消费，请重新执行预览")]
    PreviewExpired,
    /// PRD §5.2：批量链接改写失败（**已回滚**）——消息里带失败文件清单。
    #[error("批量链接改写失败（已回滚）：{0}")]
    RewriteFailed(String),
    /// 技术方案 §6.1.6：**回滚失败** —— 唯一无法保证「用户数据完好」的路径。
    ///
    /// 必须：① 专用错误（复用 `E_REWRITE_FAILED` 码，靠 context 区分，见技术方案 §13.1 的错误表写法）
    /// ② **点名哪些文件可能停在中间状态** ③ 由命令层写 ERROR 级日志。
    /// **禁止**把它静默降级成"已回滚 N 个文件"这种谎报。
    #[error("回滚失败，备份保留于 {backup_dir}；以下文件可能停在中间状态：{}", failures.join("、"))]
    RollbackFailed {
        /// 未能恢复到原文的文件（相对路径 + 原因）
        failures: Vec<String>,
        /// 备份目录（用户可据此手工恢复）
        backup_dir: String,
    },
    /// PRD §5.2 / FR-SEARCH-27 / AC-SEARCH-05：搜索语法错误必须**明说**，
    /// 不得静默返回空结果（那会让用户以为"没有匹配"），也不得抛错崩溃。
    ///
    /// 消息里**只放固定的诊断文案**（如"双引号未闭合"），不回显用户查询串：
    /// 命令层的错误出口会把 `format!("{:?}", err)` 放进 IPC 响应的 detail 字段，
    /// 拼接原查询就等于把搜索词搬到错误通道上（SEC-09 / R-12 的同一纪律）。
    #[error("搜索语法错误：{0}")]
    SearchSyntax(String),
    /// 2026-10-15 裁定①（PRD §5.3.4.3 已由"待定取舍"改为"已裁定并落地"）：
    /// **查询本身合法、但候选范围太大**（例如 line: 的候选数超过上限）与"语法写错"是**两件事**，
    /// 必须用**不同的错误码**区分 —— 否则前端只能靠 message 子串去猜，后端一改文案就静默退化成"语法提示"。
    ///
    /// 消息纪律同 SearchSyntax：只放**固定诊断文案 + 计数**，不回显用户查询串
    /// （命令层的错误出口会把 Debug 形式的错误放进 IPC 响应的 detail 字段 —— SEC-09 / R-12）。
    #[error("搜索范围过大：{0}")]
    SearchTooBroad(String),
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
            Self::DbError(_) | Self::DbErrorDetail { .. } => "E_DB_ERROR",
            Self::PreviewExpired => "E_PREVIEW_EXPIRED",
            Self::RewriteFailed(_) => "E_REWRITE_FAILED",
            // 复用码，靠 context 区分（技术方案 §13.1）
            Self::RollbackFailed { .. } => "E_REWRITE_FAILED",
            // PRD §5.2 第 1561 行：搜索语法错误 / 400 / 显示语法提示
            Self::SearchSyntax(_) => "E_SEARCH_SYNTAX",
            // PRD §5.2（2026-10-15 新增行）：搜索范围过大 / 400 / 提示缩小范围（与语法错误分开）
            Self::SearchTooBroad(_) => "E_SEARCH_TOO_BROAD",
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

    /// 与 [`AppError::db`] 同文案，但**保留底层细节**供日志使用（ERR-04）。审计 R5/G3 的修法。
    pub fn db_with(context: &str, detail: impl std::fmt::Display) -> Self {
        let message = match Self::db(context) {
            Self::DbError(m) => m,
            _ => unreachable!(),
        };
        Self::DbErrorDetail {
            message,
            detail: detail.to_string(),
        }
    }

    /// 供日志出口取用的细节（没有则为 None）。
    pub fn log_detail(&self) -> Option<&str> {
        match self {
            Self::DbErrorDetail { detail, .. } => Some(detail.as_str()),
            _ => None,
        }
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
            Self::DbError(_) | Self::DbErrorDetail { .. } => "DbError",
            Self::PreviewExpired => "PreviewExpired",
            Self::RewriteFailed(_) => "RewriteFailed",
            Self::RollbackFailed { .. } => "RollbackFailed",
            Self::SearchSyntax(_) => "SearchSyntax",
            Self::SearchTooBroad(_) => "SearchTooBroad",
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
