use serde::Serialize;

/// 最小连通性探测（M0 DoD：invoke 往返成功）。
#[tauri::command]
pub async fn ping() -> String {
    "pong".to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub platform: String,
    pub arch: String,
    pub version: String,
}

#[tauri::command]
pub async fn system_info() -> SystemInfo {
    SystemInfo {
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
