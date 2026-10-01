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
        // ERR-04：全部错误必须写入日志（错误码 + 类别 + 时间戳由日志框架补）。
        // SEC-09：此处**只记类别不记消息**——消息可能含 Vault 绝对路径等不该入日志的内容；
        // 需要更细上下文的调用点自行记录（如路径校验记相对路径）。
        tracing::warn!(code = self.0.code(), kind = self.0.kind(), "命令失败");
        let mut state = serializer.serialize_struct("KpError", 3)?;
        state.serialize_field("code", &self.0.code())?;
        state.serialize_field("message", &self.0.to_string())?;
        state.serialize_field("detail", &format!("{:?}", self.0))?;
        state.end()
    }
}
