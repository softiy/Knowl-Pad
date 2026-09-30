use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::path_guard::PathGuard;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultOpenArgs {
    pub abs_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub root: String,
    pub case_insensitive_fs: bool,
}

/// 打开 Vault：校验绝对路径可规范化，随后写入 AppState（M0 最小实现；M1 扩展为建库+索引）。
#[tauri::command]
pub async fn vault_open(
    state: tauri::State<'_, AppState>,
    args: VaultOpenArgs,
) -> Result<VaultInfo, KpError> {
    let guard = PathGuard::new(Path::new(&args.abs_path)).map_err(KpError)?;
    let root = guard.canonical_root().to_path_buf();
    state.set_root(Some(root.clone()));
    Ok(VaultInfo {
        root: root.to_string_lossy().to_string(),
        case_insensitive_fs: crate::platform::case_insensitive_fs(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub index_dir: String,
    pub index_db: String,
    pub ready: bool,
}

/// 索引状态（M0 仅回述存储布局；M1 起读取实际签名与队列深度）。
#[tauri::command]
pub async fn index_status(state: tauri::State<'_, AppState>) -> Result<IndexStatus, KpError> {
    let _guard = state.guard()?;
    Ok(IndexStatus {
        index_dir: crate::storage::INDEX_DIR_REL.to_string(),
        index_db: crate::storage::INDEX_DB_REL.to_string(),
        ready: false,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
}

/// 已注册 Vault 列表（M0 仅返回当前打开的 Vault；M1 起读 global.db 注册表）。
#[tauri::command]
pub async fn vault_list(state: tauri::State<'_, AppState>) -> Result<Vec<VaultSummary>, KpError> {
    Ok(match state.current_root() {
        Some(root) => vec![VaultSummary {
            id: 1,
            display_name: root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Vault".to_string()),
            abs_path: root.to_string_lossy().to_string(),
        }],
        None => Vec::new(),
    })
}

/// 关闭当前 Vault（M0 最小实现：清空 AppState 中的根路径；M1 起扩展为刷盘、关闭连接池与文件监听）。
#[tauri::command]
pub async fn vault_close(state: tauri::State<'_, AppState>) -> Result<(), KpError> {
    state.set_root(None);
    Ok(())
}
