use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
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

/// 打开 Vault：校验路径 → **确保索引库**（签名不一致则丢弃重建）→ 写入 AppState。
///
/// 路径校验在创建任何目录**之前**完成——路径无效时绝不产生副作用（AC-VAULT-02）。
/// 建库属阻塞 IO，放入线程池执行，避免阻塞 IPC 线程（R-08）。
#[tauri::command]
pub async fn vault_open(
    state: tauri::State<'_, AppState>,
    args: VaultOpenArgs,
) -> Result<VaultInfo, KpError> {
    let guard = PathGuard::new(Path::new(&args.abs_path)).map_err(KpError)?;
    let root = guard.canonical_root().to_path_buf();

    let root_for_db = root.clone();
    let pool =
        tauri::async_runtime::spawn_blocking(move || crate::storage::index::open(&root_for_db))
            .await
            .map_err(|err| KpError(AppError::IoFailure(format!("后台任务失败：{err}"))))?
            .map_err(KpError)?;

    state.set_index_db(Some(std::sync::Arc::new(pool)));
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
    /// 索引库 schema 版本（未就绪时为 None）
    pub schema_version: Option<i64>,
    /// 索引签名摘要前 12 位（用于 UI 诊断；未就绪时为 None）
    pub signature_prefix: Option<String>,
    /// 上次建库时间戳（毫秒）
    pub built_at: Option<i64>,
}

/// 索引状态：已打开 Vault 时读取索引库 meta；未打开时返回布局信息与 ready=false。
#[tauri::command]
pub async fn index_status(state: tauri::State<'_, AppState>) -> Result<IndexStatus, KpError> {
    let index_dir = crate::storage::INDEX_DIR_REL.to_string();
    let index_db = crate::storage::INDEX_DB_REL.to_string();

    let Some(pool) = state.index_db() else {
        return Ok(IndexStatus {
            index_dir,
            index_db,
            ready: false,
            schema_version: None,
            signature_prefix: None,
            built_at: None,
        });
    };

    let (schema_version, signature, built_at) = pool
        .with_reader(|conn| {
            Ok((
                crate::storage::index::read_meta(conn, "schema_version")?,
                crate::storage::index::read_meta(conn, "index_signature")?,
                crate::storage::index::read_meta(conn, "built_at")?,
            ))
        })
        .map_err(KpError)?;

    Ok(IndexStatus {
        index_dir,
        index_db,
        ready: true,
        schema_version: schema_version.and_then(|raw| raw.parse().ok()),
        signature_prefix: signature.map(|raw| raw.chars().take(12).collect()),
        built_at: built_at.and_then(|raw| raw.parse().ok()),
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

/// 关闭当前 Vault：释放索引库句柄并清空根路径。
///
/// 写连接由专用线程持有，句柄释放后该线程随之退出；WAL 由 SQLite 自行 checkpoint。
/// （M1 PR-3 将补齐「切换 Vault 前刷盘」的完整语义，见 AC-VAULT-03。）
#[tauri::command]
pub async fn vault_close(state: tauri::State<'_, AppState>) -> Result<(), KpError> {
    state.set_index_db(None);
    state.set_root(None);
    Ok(())
}
