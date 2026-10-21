//! 索引域命令（PRD §5.3.7）。
//! M3 WP1 先落两个**诊断类**命令（可直接实现、无副作用）：签名信息与实体计数。
//! `index_rebuild` / `index_cancel` 依赖索引引擎（M3 WP4），随该工作包落地。

use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

use crate::commands::paths::root_of;
use crate::error_wrapper::KpError;
use crate::index_engine::{full_index_cancellable, IndexMode, IndexOutcome};
use crate::state::AppState;
use crate::storage::index::{clear_meta, set_meta, IndexSignature, META_REBUILD_IN_PROGRESS};

/// 签名字段视图（PRD §5.3.7 的 `SignatureInfo`）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureParts {
    pub schema_version: u32,
    pub parser_version: u32,
    pub tokenizer_version: String,
    pub tokenizer_dict_hash: String,
    pub vault_root: String,
    pub digest: String,
}

impl SignatureParts {
    fn from(sig: &IndexSignature) -> Self {
        Self {
            schema_version: sig.schema_version,
            parser_version: sig.parser_version,
            tokenizer_version: sig.tokenizer_version.clone(),
            tokenizer_dict_hash: sig.tokenizer_dict_hash.clone(),
            vault_root: sig.vault_root.clone(),
            digest: sig.digest.clone(),
        }
    }
}

/// 签名诊断信息（`index_signature_get`）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    /// 按当前版本常量与 Vault 根算出的**期望**签名。
    pub expected: SignatureParts,
    /// 索引库里实际记录的签名（未建库/未写过时为 None）。
    pub stored: Option<SignatureParts>,
    /// 两者是否一致（stored 为 None 时恒为 false，语义同「不可信即重建」）。
    pub matched: bool,
    /// 不一致的原因（诊断用；一致时为 None）。
    pub reason: Option<String>,
}

/// 各类实体计数（`index_stats`；设置页展示用，PRD §5.3.7）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub files: i64,
    pub deleted_files: i64,
    pub links: i64,
    pub tags: i64,
    pub headings: i64,
    pub block_ids: i64,
    pub fts_rows: i64,
}

/// 读取各表实体计数（脱离 Tauri State，便于用真实临时索引库单测）。
pub(crate) fn read_stats(
    pool: &crate::storage::pool::DbPool,
) -> Result<IndexStats, kp_domain::error::AppError> {
    pool.with_reader(|conn| {
        let count = |sql: &str| -> Result<i64, kp_domain::error::AppError> {
            conn.query_row(sql, [], |row| row.get::<_, i64>(0))
                .map_err(|e| kp_domain::error::AppError::db_with("统计索引实体", &e))
        };
        Ok(IndexStats {
            files: count("SELECT count(*) FROM file WHERE deleted = 0")?,
            deleted_files: count("SELECT count(*) FROM file WHERE deleted = 1")?,
            links: count("SELECT count(*) FROM link")?,
            tags: count("SELECT count(*) FROM tag")?,
            headings: count("SELECT count(*) FROM heading")?,
            block_ids: count("SELECT count(*) FROM block_id")?,
            fts_rows: count("SELECT count(*) FROM note_fts")?,
        })
    })
}

/// 签名对比（与 Tauri State 无关，供命令层与打开 Vault 的对账路径共用）。
pub(crate) fn signature_info(
    root: &std::path::Path,
    pool: Option<&crate::storage::pool::DbPool>,
) -> SignatureInfo {
    let expected = IndexSignature::current(root);
    // M3 复核 M3：此前把 meta 里的 64 位十六进制 digest 当 JSON 解析 → 恒 None →
    let stored = pool.and_then(|p| {
        p.with_reader(crate::storage::index::stored_signature)
            .ok()
            .flatten()
    });
    let matched = stored.as_ref().is_some_and(|s| s.digest == expected.digest);
    let reason = if matched {
        None
    } else {
        Some(match &stored {
            None => "索引库中还没有签名记录（首次打开或索引尚未完成），将触发全量索引".to_string(),
            Some(s) => s.diff_reason(&expected),
        })
    };
    SignatureInfo {
        expected: SignatureParts::from(&expected),
        stored: stored.as_ref().map(SignatureParts::from),
        matched,
        reason,
    }
}

/// 当前索引签名与磁盘上记录的签名对比（FR-SIG-01 的诊断入口）。
#[tauri::command]
pub async fn index_signature_get(state: State<'_, AppState>) -> Result<SignatureInfo, KpError> {
    let root = root_of(&state)?;
    let pool = state.index_db();
    Ok(signature_info(&root, pool.as_deref()))
}

/// 各表实体计数（`index_stats`）。
#[tauri::command]
pub async fn index_stats(state: State<'_, AppState>) -> Result<IndexStats, KpError> {
    index_stats_impl(&state)
}

/// 命令体（把状态作为参数传入，便于测试 —— 项目既有做法，见 DEBT-14）。
///
/// 审计 T-03：守卫此前内联在 `#[tauri::command]` 里，单测**无法**调用（Tauri 的 `State` 造不出来），
/// 于是那条用例只能自己构造 `AppError::VaultNotOpen` 再断言错误码 —— 名字说"stats 需要打开 Vault"，
/// 实际从没验证过命令的守卫。抽出来后守卫才真正被用例覆盖。
pub(crate) fn index_stats_impl(state: &AppState) -> Result<IndexStats, KpError> {
    let pool = state
        .index_db()
        .ok_or(KpError(kp_domain::error::AppError::VaultNotOpen))?;
    read_stats(&pool).map_err(KpError)
}

// ── 索引编排（FR-VAULT-09 / AC-VAULT-05；事件载荷以 PRD §5.4 为准，勘误 D-23）──────────

/// kp://index/progress 载荷（节流 ≥100ms）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub current_file: Option<String>,
}

/// kp://index/completed 载荷。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexCompleted {
    pub stats: IndexStatsPayload,
    pub duration_ms: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatsPayload {
    pub indexed: usize,
    pub skipped: usize,
    pub cancelled: bool,
}

/// kp://index/failed 载荷（**无 detail 字段**：以 PRD 为准，勘误 D-23）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexFailed {
    pub code: String,
    pub message: String,
    pub failed_files: Vec<String>,
}

/// index_rebuild 的返回值。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildResult {
    pub indexed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub cancelled: bool,
    pub warnings: Vec<String>,
}

/// `kp://index/cancelled` 的载荷（PRD §5.4）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexCancelled {
    pub stats: IndexStatsPayload,
    pub duration_ms: u64,
}

/// 一次重建的**收尾判定**（DEBT-45）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RebuildEnding {
    Completed,
    Cancelled,
}

/// 收尾判定对应的**唯一事件名**（命令按它分发；用例据此钉住"取消不发 completed"）。
pub(crate) fn ending_event(ending: RebuildEnding) -> &'static str {
    match ending {
        RebuildEnding::Completed => "kp://index/completed",
        RebuildEnding::Cancelled => "kp://index/cancelled",
    }
}

/// **DEBT-45 的修复本体**：取消 ≠ 完成。
///
/// 为什么必须这样：`full_index_cancellable` 按批 COMMIT（BATCH_SIZE=200），取消发生在批间 ⇒ 已提交的批次
/// **留在库里**；若此时仍 `set_index_ready(true)`，UI 与后续命令会把"半索引"当完整索引用（比如搜索缺结果、
/// 反向链接缺边），而用户以为索引好了。签名 meta 由**打开路径**写入、取消不会让它失效，所以必须显式留下
/// `rebuild_in_progress` 标记 —— `ensure_index_db` 见到它就判 `Rebuilt`（index_rebuild.rs），下次打开必重建。
pub(crate) fn settle_rebuild(
    state: &AppState,
    pool: &crate::storage::pool::DbPool,
    outcome: &IndexOutcome,
) -> Result<RebuildEnding, kp_domain::error::AppError> {
    if outcome.cancelled {
        state.set_index_ready(false);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let stamp = now.to_string();
        pool.with_writer(move |conn| set_meta(conn, META_REBUILD_IN_PROGRESS, &stamp))?;
        return Ok(RebuildEnding::Cancelled);
    }
    // 成功：清掉可能残留的标记（上一次取消留下的），再宣称就绪
    pool.with_writer(|conn| clear_meta(conn, META_REBUILD_IN_PROGRESS))?;
    state.set_index_ready(true);
    Ok(RebuildEnding::Completed)
}

/// 索引完成后启动监听（失败则退化为 5s 轮询对账，NFR-PLAT-09）。
///
/// 从 `index_rebuild` 抽出：① 这一段的失败语义独立（降级而非失败）；② 命令本体要控制在 clippy 的行数线内。
fn ensure_watcher(
    state: &AppState,
    pool: &std::sync::Arc<crate::storage::pool::DbPool>,
    root: &std::path::Path,
    app: &AppHandle,
) {
    if !state.has_watcher_for(root) {
        state.stop_watcher();
    } // 按 root 判定并先停旧监听
    if state.has_watcher_for(root) {
        return;
    }
    match crate::index_watch::start_watcher(pool.clone(), root.to_path_buf(), app.clone()) {
        Ok(handle) => state.set_watcher(handle, root.to_path_buf()),
        Err(err) => {
            tracing::warn!(error = %err, "文件监听启动失败，退化为轮询对账");
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            state.set_fallback_stop(stop.clone()); // 审计 T-21：登记停止标志，切库/关闭时置位
            crate::index_watch::start_polling_fallback(pool.clone(), root.to_path_buf(), stop);
        }
    }
}

/// 全量重建索引（force = false 时跳过 mtime 与大小都未变的文件）。
#[tauri::command]
pub async fn index_rebuild(
    force: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<IndexRebuildResult, KpError> {
    let root = root_of(&state)?;
    let pool = state
        .index_db()
        .ok_or(KpError(kp_domain::error::AppError::VaultNotOpen))?;
    // 清掉历史取消请求，避免上一次的取消影响本次
    state.take_index_cancel();
    let mode = if force {
        IndexMode::Force
    } else {
        IndexMode::SkipUnchanged
    };
    let mut last = Instant::now();
    let progress_app = app.clone();
    let result = full_index_cancellable(
        &pool,
        &root,
        move |done, total| {
            if last.elapsed().as_millis() >= 100 || done == total {
                let _ = progress_app.emit(
                    "kp://index/progress",
                    IndexProgress {
                        phase: "full".to_string(),
                        done,
                        total,
                        current_file: None,
                    },
                );
                last = Instant::now();
            }
        },
        mode,
        || state.take_index_cancel(),
    );
    match result {
        Ok(outcome) => {
            let ending = settle_rebuild(&state, &pool, &outcome)?;
            let stats = IndexStatsPayload {
                indexed: outcome.indexed,
                skipped: outcome.skipped,
                cancelled: outcome.cancelled,
            };
            if ending == RebuildEnding::Cancelled {
                // 取消：**不**宣称就绪（settle_rebuild 已置 false）、**不**启动监听（否则会往半索引上叠增量）、
                // **不**发 completed（前端据此隐藏进度并提示"下次打开会重建"）。
                let _ = app.emit(
                    ending_event(ending),
                    IndexCancelled {
                        stats,
                        duration_ms: outcome.duration_ms,
                    },
                );
                return Ok(IndexRebuildResult {
                    indexed: outcome.indexed,
                    skipped: outcome.skipped,
                    duration_ms: outcome.duration_ms,
                    cancelled: true,
                    warnings: outcome.warnings,
                });
            }
            ensure_watcher(&state, &pool, &root, &app);
            let _ = app.emit(
                ending_event(ending),
                IndexCompleted {
                    stats,
                    duration_ms: outcome.duration_ms,
                },
            );
            Ok(IndexRebuildResult {
                indexed: outcome.indexed,
                skipped: outcome.skipped,
                duration_ms: outcome.duration_ms,
                cancelled: outcome.cancelled,
                warnings: outcome.warnings,
            })
        }
        Err(err) => {
            let _ = app.emit(
                "kp://index/failed",
                IndexFailed {
                    code: err.code().to_string(),
                    message: err.to_string(),
                    failed_files: Vec::new(),
                },
            );
            Err(KpError(err))
        }
    }
}

/// 请求取消当前索引（幂等；索引循环在批间检查，AC-VAULT-05）。
#[tauri::command]
pub async fn index_cancel(state: State<'_, AppState>) -> Result<bool, KpError> {
    state.request_index_cancel();
    Ok(true)
}
