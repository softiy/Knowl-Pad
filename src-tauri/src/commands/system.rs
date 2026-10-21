use crate::state::AppState;
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
    /// **FR-GLOBAL-01 / DEBT-20**：本次启动是否因全局库不可用而重建（正常启动恒 `false`）。
    pub global_db_rebuilt: bool,
    /// 原库被保留成的**文件名**（不含目录；未保留/未重建时为 None）。
    pub global_db_backup: Option<String>,
    /// 重建原因摘要（中文可读；正常启动为 None）。
    pub global_db_reason: Option<String>,
}

/// 平台/版本信息（PRD §5.3.9）。
///
/// **2026-10-21（DEBT-20）**：顺带承担"全局库已重建"的一次性告知 —— 恢复信息只在启动时产生一次，
/// 前端在布局挂载时取一次即可，不需要新事件（事件可能在 webview 就绪前发出而丢失）。
// 同步命令：读取内存里的恢复标记，没有 IO（async + `State<'_>` 会被 Tauri 要求返回 Result，
// 而这里"取不到"不是错误 —— 保持无错误的同步签名更贴合语义）。
#[tauri::command]
pub fn system_info(state: tauri::State<'_, AppState>) -> SystemInfo {
    let recovery = state.global_recovery();
    SystemInfo {
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        global_db_rebuilt: recovery.is_some(),
        global_db_backup: recovery.as_ref().and_then(|r| r.preserved_file.clone()),
        global_db_reason: recovery.map(|r| r.reason),
    }
}
