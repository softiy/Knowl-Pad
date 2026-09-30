use kp_domain::error::AppError;
use serde::ser::SerializeStruct;

/// 包装领域错误以满足 Tauri 命令的 Serialize 要求（输出 PRD §5.2 的 KpError 结构）。
#[derive(Debug)]
pub struct KpError(pub AppError);

impl From<AppError> for KpError {
    fn from(value: AppError) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for KpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for KpError {}

impl serde::Serialize for KpError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("KpError", 3)?;
        state.serialize_field("code", &self.0.code())?;
        state.serialize_field("message", &self.0.to_string())?;
        state.serialize_field("detail", &format!("{:?}", self.0))?;
        state.end()
    }
}
