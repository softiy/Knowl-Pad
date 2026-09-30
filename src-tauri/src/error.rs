use thiserror::Error;

/// 统一错误类型：全部对外错误收敛于此，并映射为 PRD §5.2 的错误码（RS-04）。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("当前没有打开的知识库")]
    VaultNotOpen,
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
    #[error("读写失败：{0}")]
    IoFailure(String),
}

impl AppError {
    /// 映射为 PRD §5.2 的错误码（前端契约）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::VaultNotOpen => "E_VAULT_NOT_OPEN",
            Self::PathEmpty | Self::PathEscapeDeny => "E_PATH_ESCAPE_DENY",
            Self::PathAbsolute | Self::PathOutsideVault => "E_PATH_OUTSIDE_VAULT",
            Self::InvalidFilename(_) => "E_INVALID_FILENAME",
            Self::FileNotFound(_) => "E_FILE_NOT_FOUND",
            Self::FileExists(_) => "E_FILE_EXISTS",
            Self::WriteConflict(_) => "E_WRITE_CONFLICT",
            Self::IoFailure(_) => "E_IO_FAILURE",
        }
    }
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("KpError", 3)?;
        state.serialize_field("code", &self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("detail", &format!("{self:?}"))?;
        state.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::NotFound {
            Self::FileNotFound(err.to_string())
        } else {
            Self::IoFailure(err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(AppError::VaultNotOpen.code(), "E_VAULT_NOT_OPEN");
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
    }

    #[test]
    fn io_errors_map_by_kind() {
        let nf = AppError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert_eq!(nf.code(), "E_FILE_NOT_FOUND");
        let other = AppError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "deny",
        ));
        assert_eq!(other.code(), "E_IO_FAILURE");
    }
}
