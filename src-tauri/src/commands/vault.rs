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
        // AC-REL-02：**对账完成前不得宣称就绪**（此前 Unchanged 在打开时就置 true，UI 会先显示「就绪」）。
        let state = app.state::<AppState>();
        let result = run_reconcile(&state, &pool, &root, progress);
        match result {
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
                // AC-REL-02：对账失败 ⇒ **不启动监听**（否则增量会建在半索引上），就绪保持 false。
                // 用户能从 kp://index/failed 与「未就绪」状态看出来；下次打开会因标记而重建。
                return;
            }
        }
        // M3 复核 blocker 修复：监听是「每个 Vault 一个」。此前用不带 root 的
        if !state.has_watcher_for(&root) {
            state.stop_watcher();
            match start_watcher(pool.clone(), root.clone(), app.clone()) {
                Ok(handle) => state.set_watcher(handle, root.clone()),
                Err(err) => {
                    tracing::warn!(error = %err, "文件监听启动失败，退化为轮询对账");
                    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    state.set_fallback_stop(stop.clone()); // 审计 T-21：登记停止标志，切库/关闭时置位
                    crate::index_watch::start_polling_fallback(pool.clone(), root.clone(), stop);
                }
            }
        }
    });
}

/// 打开 Vault 后的**对账本体**（可单测，不依赖 Tauri 事件/句柄）。
///
/// AC-REL-02：就绪信号反映「对账是否完成」—— 开始前置 false，成功后才 true；失败保持 false
/// （调用方据此**不启动监听**，见 `spawn_reconcile_and_watch`）。
pub(crate) fn run_reconcile<P: FnMut(usize, usize)>(
    state: &AppState,
    pool: &crate::storage::pool::DbPool,
    root: &std::path::Path,
    progress: P,
) -> Result<crate::index_engine::IndexOutcome, kp_domain::error::AppError> {
    state.set_index_ready(false);
    // 新一轮对账开始 ⇒ 先清掉上一个 Vault / 上一轮的警告（**打开新 Vault 时清空**的同一处保证：
    // 打开与启动恢复都经这里；失败时保持为空，避免把历史警告当成本次结果）。
    state.set_index_warnings(Vec::new());
    let result = full_index_cancellable(pool, root, progress, IndexMode::SkipUnchanged, || false);
    if let Ok(outcome) = &result {
        state.set_index_ready(true);
        // **T1 根因修复**（SEC-03 / 勘误 D-46）：outcome.warnings（skip-timeout:/skip-oversize:/遍历告警）
        // 此前在本路径被丢弃 —— 事件载荷按 PRD §5.4 不得扩展，故改由 AppState 经 index_status 透出。
        state.set_index_warnings(outcome.warnings.clone());
    }
    result
}

/// 阅读态本地附件（E1 / FR-EDITOR-43 / SEC-08，task-2 跨域接线）：把**当前** Vault 根目录
/// 加入 asset 协议读取范围。
///
/// 为什么运行时授权而不是写死 assetProtocol.scope：Vault 路径由用户任选，静态配置无法预先枚举；
/// 只开放当前打开的那一个 Vault，也符合 SEC-05 的最小授权口径。
fn allow_vault_assets(app: &AppHandle, root: &std::path::Path) {
    if let Err(err) = app.asset_protocol_scope().allow_directory(root, true) {
        tracing::warn!(error = %err, "授予本地附件读取权限失败");
    }
}

/// 关闭 / 切走 Vault 时收回（否则切库后旧库的图片仍可被 asset URL 读取）。
fn forbid_vault_assets(app: &AppHandle, root: &std::path::Path) {
    if let Err(err) = app.asset_protocol_scope().forbid_directory(root, true) {
        tracing::warn!(error = %err, "收回本地附件读取权限失败");
    }
}

/// 从状态里取根与索引库；两者齐备才启动后台任务。
pub(crate) fn reconcile_after_open(state: &AppState, app: &AppHandle) {
    let Ok(root) = super::paths::root_of(state) else {
        return;
    };
    // E1：asset 协议只信任运行时登记的目录 —— 打开 / 新建 / 启动恢复都经这里，统一授权避免三处遗漏。
    allow_vault_assets(app, &root);
    let Some(pool) = state.index_db() else {
        return;
    };
    // 切库瞬间就作废旧警告（不能等后台对账真的跑起来 —— 那之前一次 index_status 会显示上一个 Vault 的警告）。
    state.set_index_warnings(Vec::new());
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
    // E1：切换 Vault 先记下旧根，激活后收回旧根的 asset 授权（reconcile_after_open 负责授予新根）。
    let previous_root = state.current_root();
    let info = activate_vault(&state, prepared).map_err(KpError)?;
    if let Some(old_root) = previous_root {
        forbid_vault_assets(&app, &old_root);
    }
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
    // E1：同 vault_open —— 新库授权由 reconcile_after_open 统一做，旧库授权在这里收回。
    let previous_root = state.current_root();
    let info = activate_vault(&state, prepared).map_err(KpError)?;
    if let Some(old_root) = previous_root {
        forbid_vault_assets(&app, &old_root);
    }
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
pub async fn vault_close(state: tauri::State<'_, AppState>, app: AppHandle) -> Result<(), KpError> {
    // 先停止文件监听（M3 WP5），避免关闭过程中的事件触发增量写库
    state.stop_watcher();
    let previous_root = state.current_root();
    close_current(&state);
    // E1（task-2 跨域接线）：关闭后收回 asset 读取授权，否则已关闭 Vault 的图片仍可被 asset URL 读取。
    if let Some(old_root) = previous_root {
        forbid_vault_assets(&app, &old_root);
    }
    // 没有 Vault 就没有"最近一次索引"，旧警告不得残留到下一个 Vault / 空状态。
    state.set_index_warnings(Vec::new());
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
    app: AppHandle,
    args: VaultIdArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::remove_vault(&global, args.vault_id).map_err(KpError)?;
    // 若移除的是当前打开的 Vault，则同时关闭（但不触碰磁盘）
    if state.current_vault_id() == Some(args.vault_id) {
        let previous_root = state.current_root();
        close_current(&state);
        // E1（task-2 跨域接线）：关闭即收回 asset 授权。
        if let Some(old_root) = previous_root {
            forbid_vault_assets(&app, &old_root);
        }
        state.set_index_warnings(Vec::new());
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
    app: AppHandle,
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
    let previous_root = state.current_root();
    let mut info = activate_vault(&state, prepared).map_err(KpError)?;
    info.vault_id = Some(args.vault_id);
    state.set_current_vault_id(Some(args.vault_id));
    // E1（task-2 跨域接线）：换根后收回旧根 asset 授权、授予新根 —— 本路径不触发对账，授权必须显式做。
    if let Some(old_root) = previous_root {
        forbid_vault_assets(&app, &old_root);
    }
    if let Some(root) = state.current_root() {
        allow_vault_assets(&app, &root);
    }
    // 换根 ⇒ 上一个根的警告作废（本路径不触发对账，等下次打开/重建再产生新警告）。
    state.set_index_warnings(Vec::new());
    Ok(info)
}

/// 索引状态：已打开 Vault 时读取索引库 meta；未打开时返回布局信息与 ready=false。
///
/// 命令体抽成可注入状态的函数（与 index_stats_impl 同一做法，见 DEBT-14 / 审计 T-03）：
/// 「警告透出」是本缺陷的**契约面**，必须能脱离 Tauri State 直接断言（T1 / 勘误 D-46）。
pub(crate) fn index_status_impl(state: &AppState) -> Result<IndexStatus, KpError> {
    let index_dir = crate::storage::INDEX_DIR_REL.to_string();
    let index_db = crate::storage::INDEX_DB_REL.to_string();
    let warnings = state.index_warnings();

    let Some(pool) = state.index_db() else {
        return Ok(IndexStatus {
            index_dir,
            index_db,
            ready: false,
            schema_version: None,
            signature_prefix: None,
            built_at: None,
            warnings,
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
        warnings,
    })
}

/// 索引状态（薄壳，RS-01）：实现见 index_status_impl。
#[tauri::command]
pub async fn index_status(state: tauri::State<'_, AppState>) -> Result<IndexStatus, KpError> {
    index_status_impl(&state)
}

#[cfg(test)]
#[path = "vault_tests.rs"]
mod tests;
