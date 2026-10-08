//! Vault 相关命令（薄壳，RS-01）：实现见 vault_lifecycle.rs，数据结构见 vault_types.rs。

use super::vault_lifecycle::{activate_vault, close_current, current_vault, prepare_vault};
use super::vault_types::{
    IndexStatus, VaultCreateArgs, VaultIdArgs, VaultInfo, VaultOpenArgs, VaultPinArgs,
    VaultRelocateArgs, VaultRenameArgs, VaultSummary,
};
use crate::error_wrapper::KpError;
use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::index_watch::start_watcher;
use crate::state::AppState;
use kp_domain::error::AppError;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// 打开/新建 Vault 后的后台任务：**先做 mtime 对账**（跳过未变更），**再启动文件监听**。
/// 监听启动失败（如 inotify watch 耗尽）则退化为 5s 轮询对账（NFR-PLAT-09）。
fn spawn_reconcile_and_watch(
    app: AppHandle,
    root: PathBuf,
    pool: std::sync::Arc<crate::storage::pool::DbPool>,
) {
    tauri::async_runtime::spawn(async move {
        // FR-SIG-01：签名不匹配（或缺失）时**先发 kp://index/rebuild-required**，
        let sig = crate::commands::index::signature_info(&root, Some(&pool));
        if !sig.matched {
            let _ = app.emit(
                "kp://index/rebuild-required",
                serde_json::json!({
                    "reason": sig.reason.clone().unwrap_or_default(),
                    "oldSig": sig.stored.as_ref().map(|s| s.digest.clone()),
                    "newSig": sig.expected.digest.clone(),
                }),
            );
        }
        // FR-VAULT-09 / D-18：打开 Vault 的对账也要发进度（节流 ≥100ms，形状按 PRD §5.4）。
        // 此前传 |_, _| {}，导致进度事件只有 index_rebuild 才会发（M3 复核 M8）。
        let progress_app = app.clone();
        let last_emit =
            std::cell::Cell::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
        let progress = move |done: usize, total: usize| {
            if last_emit.get().elapsed() < std::time::Duration::from_millis(100) {
                return;
            }
            last_emit.set(std::time::Instant::now());
            let _ = progress_app.emit(
                "kp://index/progress",
                serde_json::json!({
                    "phase": "full",
                    "done": done,
                    "total": total,
                    "currentFile": serde_json::Value::Null,
                }),
            );
        };
        match full_index_cancellable(&pool, &root, progress, IndexMode::SkipUnchanged, || false) {
            Ok(outcome) => {
                let _ = app.emit(
                    "kp://index/completed",
                    crate::commands::index::IndexCompleted {
                        stats: crate::commands::index::IndexStatsPayload {
                            indexed: outcome.indexed,
                            skipped: outcome.skipped,
                            cancelled: outcome.cancelled,
                        },
                        duration_ms: outcome.duration_ms,
                    },
                );
            }
            Err(err) => {
                tracing::warn!(error = %err, "打开 Vault 后的对账失败");
                let _ = app.emit(
                    "kp://index/failed",
                    crate::commands::index::IndexFailed {
                        code: err.code().to_string(),
                        message: err.to_string(),
                        failed_files: Vec::new(),
                    },
                );
            }
        }
        let state = app.state::<AppState>();
        // M3 复核 blocker 修复：监听是「每个 Vault 一个」。此前用不带 root 的
        if !state.has_watcher_for(&root) {
            state.stop_watcher();
            match start_watcher(pool.clone(), root.clone(), app.clone()) {
                Ok(handle) => state.set_watcher(handle, root.clone()),
                Err(err) => {
                    tracing::warn!(error = %err, "文件监听启动失败，退化为轮询对账");
                    crate::index_watch::start_polling_fallback(pool.clone(), root.clone());
                }
            }
        }
    });
}

/// 从状态里取根与索引库；两者齐备才启动后台任务。
pub(crate) fn reconcile_after_open(state: &AppState, app: &AppHandle) {
    let Ok(root) = super::paths::root_of(state) else {
        return;
    };
    let Some(pool) = state.index_db() else {
        return;
    };
    spawn_reconcile_and_watch(app.clone(), root, pool);
}
// ── 命令（薄壳，RS-01）────────────────────────────────────────────

/// 打开已有文件夹为 Vault（路径必须存在；否则 E_VAULT_PATH_INVALID 且不创建目录）。
#[tauri::command]
pub async fn vault_open(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    args: VaultOpenArgs,
) -> Result<VaultInfo, KpError> {
    let path = PathBuf::from(&args.abs_path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_vault(&path, false))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "后台任务失败");
            KpError(AppError::IoFailure(
                "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
            ))
        })?
        .map_err(KpError)?;
    let info = activate_vault(&state, prepared).map_err(KpError)?;
    reconcile_after_open(&state, &app);
    Ok(info)
}

/// 新建 Vault：路径不存在时创建目录，并在其中初始化 .knowlpad/ 与空索引库。
#[tauri::command]
pub async fn vault_create(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    args: VaultCreateArgs,
) -> Result<VaultInfo, KpError> {
    let path = PathBuf::from(&args.abs_path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_vault(&path, true))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "后台任务失败");
            KpError(AppError::IoFailure(
                "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
            ))
        })?
        .map_err(KpError)?;
    let info = activate_vault(&state, prepared).map_err(KpError)?;
    reconcile_after_open(&state, &app);
    if let (Some(name), Some(id), Some(global)) = (args.name, info.vault_id, state.global_db()) {
        crate::storage::global::rename_vault(&global, id, &name).map_err(KpError)?;
        return Ok(VaultInfo {
            display_name: name,
            ..info
        });
    }
    Ok(info)
}

/// 关闭当前 Vault（先刷盘）。
#[tauri::command]
pub async fn vault_close(state: tauri::State<'_, AppState>) -> Result<(), KpError> {
    // 先停止文件监听（M3 WP5），避免关闭过程中的事件触发增量写库
    state.stop_watcher();
    close_current(&state);
    Ok(())
}

/// 当前 Vault 信息（未打开时返回 null）。
#[tauri::command]
pub async fn vault_current(
    state: tauri::State<'_, AppState>,
) -> Result<Option<VaultInfo>, KpError> {
    Ok(current_vault(&state))
}

/// 已注册 Vault 列表（来自全局库；置顶优先，其次最近打开）。
#[tauri::command]
pub async fn vault_list(state: tauri::State<'_, AppState>) -> Result<Vec<VaultSummary>, KpError> {
    let Some(global) = state.global_db() else {
        return Ok(Vec::new());
    };
    let rows = crate::storage::global::list_vaults(&global).map_err(KpError)?;
    Ok(rows
        .into_iter()
        .map(|row| VaultSummary {
            id: row.id,
            abs_path: row.abs_path,
            display_name: row.display_name,
            last_opened: row.last_opened,
            pinned: row.pinned,
        })
        .collect())
}

/// 从列表移除：**仅删除注册记录，绝不删除磁盘文件**（AC-VAULT-04）。
#[tauri::command]
pub async fn vault_register_remove(
    state: tauri::State<'_, AppState>,
    args: VaultIdArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::remove_vault(&global, args.vault_id).map_err(KpError)?;
    // 若移除的是当前打开的 Vault，则同时关闭（但不触碰磁盘）
    if state.current_vault_id() == Some(args.vault_id) {
        close_current(&state);
    }
    Ok(())
}

/// 置顶 / 取消置顶（FR-VAULT-07；PRD 勘误 D-10 补齐的命令契约）。
#[tauri::command]
pub async fn vault_pin(
    state: tauri::State<'_, AppState>,
    args: VaultPinArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::set_pinned(&global, args.vault_id, args.pinned).map_err(KpError)?;
    Ok(())
}

/// 修改显示名。
#[tauri::command]
pub async fn vault_rename(
    state: tauri::State<'_, AppState>,
    args: VaultRenameArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::rename_vault(&global, args.vault_id, &args.display_name)
        .map_err(KpError)?;
    Ok(())
}

/// 路径失效后重新定位（FR-VAULT-08）：新路径必须存在且可访问。
#[tauri::command]
pub async fn vault_relocate(
    state: tauri::State<'_, AppState>,
    args: VaultRelocateArgs,
) -> Result<VaultInfo, KpError> {
    let path = PathBuf::from(&args.new_abs_path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_vault(&path, false))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "后台任务失败");
            KpError(AppError::IoFailure(
                "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
            ))
        })?
        .map_err(KpError)?;

    let abs_path = prepared.root.to_string_lossy().to_string();
    if let Some(global) = state.global_db() {
        crate::storage::global::relocate_vault(&global, args.vault_id, &abs_path)
            .map_err(KpError)?;
    }
    let mut info = activate_vault(&state, prepared).map_err(KpError)?;
    info.vault_id = Some(args.vault_id);
    state.set_current_vault_id(Some(args.vault_id));
    Ok(info)
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
        // M3 复核 M3-8：ready 表示**索引可用**，判据来自存储层（见 state::index_ready）。
        ready: state.index_ready(),
        schema_version: schema_version.and_then(|raw| raw.parse().ok()),
        signature_prefix: signature.map(|raw| raw.chars().take(12).collect()),
        built_at: built_at.and_then(|raw| raw.parse().ok()),
    })
}

#[cfg(test)]
#[path = "vault_tests.rs"]
mod tests;
