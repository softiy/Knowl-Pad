//! 索引域命令（PRD §5.3.7）。
//!
//! M3 WP1 先落两个**诊断类**命令（可直接实现、无副作用）：签名信息与实体计数。
//! `index_rebuild` / `index_cancel` 依赖索引引擎（M3 WP4），随该工作包落地。

use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

use crate::commands::paths::root_of;
use crate::error_wrapper::KpError;
use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::state::AppState;
use crate::storage::index::{self, IndexSignature};

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

/// 解析存储侧写入的签名 JSON。**不可信即 None**（不猜测、不放过）——纯函数，便于单测。
pub(crate) fn parse_stored_signature(raw: Option<&str>) -> Option<SignatureParts> {
    let value: serde_json::Value = serde_json::from_str(raw?).ok()?;
    Some(SignatureParts {
        schema_version: value.get("schema_version")?.as_u64()? as u32,
        parser_version: value.get("parser_version")?.as_u64()? as u32,
        tokenizer_version: value.get("tokenizer_version")?.as_str()?.to_string(),
        tokenizer_dict_hash: value.get("tokenizer_dict_hash")?.as_str()?.to_string(),
        vault_root: value.get("vault_root")?.as_str()?.to_string(),
        digest: value.get("digest")?.as_str()?.to_string(),
    })
}

/// 读取各表实体计数（脱离 Tauri State，便于用真实临时索引库单测）。
pub(crate) fn read_stats(
    pool: &crate::storage::pool::DbPool,
) -> Result<IndexStats, kp_domain::error::AppError> {
    pool.with_reader(|conn| {
        let count = |sql: &str| -> Result<i64, kp_domain::error::AppError> {
            conn.query_row(sql, [], |row| row.get::<_, i64>(0))
                .map_err(|_| kp_domain::error::AppError::db("统计索引实体"))
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

/// 当前索引签名与磁盘上记录的签名对比（FR-SIG-01 的诊断入口）。
#[tauri::command]
pub async fn index_signature_get(state: State<'_, AppState>) -> Result<SignatureInfo, KpError> {
    let root = root_of(&state)?;
    let expected = IndexSignature::current(&root);
    let pool = state.index_db();
    let stored_raw = match &pool {
        Some(pool) => pool
            .with_reader(|conn| index::read_meta(conn, "index_signature"))
            .map_err(KpError)
            .ok()
            .flatten(),
        None => None,
    };
    let stored = parse_stored_signature(stored_raw.as_deref());
    let matched = stored.as_ref().is_some_and(|s| s.digest == expected.digest);
    let reason = if matched {
        None
    } else if stored.is_none() {
        Some(if stored_raw.is_none() {
            "索引库中还没有签名记录（首次打开或索引尚未完成），将触发全量索引".to_string()
        } else {
            "索引库中的签名无法解析，按不可信处理，将触发全量索引".to_string()
        })
    } else {
        Some(stored_raw.unwrap_or_default())
    };
    Ok(SignatureInfo {
        expected: SignatureParts::from(&expected),
        stored,
        matched,
        reason,
    })
}

/// 各表实体计数（`index_stats`）。
#[tauri::command]
pub async fn index_stats(state: State<'_, AppState>) -> Result<IndexStats, KpError> {
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
            // 索引完成后再启动监听：确保监听期间的增量写库建立在完整索引之上。
            // 监听启动失败（如 inotify watch 耗尽）则退化为 5s 轮询对账（NFR-PLAT-09）。
            if !state.has_watcher() {
                match crate::index_watch::start_watcher(pool.clone(), root.clone(), app.clone()) {
                    Ok(handle) => state.set_watcher(handle),
                    Err(err) => {
                        tracing::warn!(error = %err, "文件监听启动失败，退化为轮询对账");
                        crate::index_watch::start_polling_fallback(pool.clone(), root.clone());
                    }
                }
            }
            let _ = app.emit(
                "kp://index/completed",
                IndexCompleted {
                    stats: IndexStatsPayload {
                        indexed: outcome.indexed,
                        skipped: outcome.skipped,
                        cancelled: outcome.cancelled,
                    },
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
