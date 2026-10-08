//! `link_resolve_ambiguous`（PRD §5.3.5；FR-LINK-22 / AC-LINK-05）。
//!
//! 用户为**一条**歧义链接指定目标后，把它改写为**完整相对路径**（PRD:1032 明文），
//! 并走与批量改写相同的保障：**备份 → 原子写 → 可回滚**（复用 #86/#90 的写入路径与操作登记）。
//!
//! 之所以单独一个文件：它只改**一处**，与批量改写（link_rewrite.rs 的两阶段）在语义与
//! 入参上都不同；放在一起会把那个文件顶过 CODE-11 的 200 行线。

use serde::Serialize;
use tauri::State;

use crate::commands::link_rewrite_store::{next_id, put_operation, StoredOperation};
use crate::commands::paths::root_of;
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::link_rewrite_apply::{apply, FileChange, RewritePlan};
use kp_domain::link_rewrite_single::rewrite_occurrence_at;

/// 与 PRD §5.3.3 的 `RewriteResult` 同形（歧义消解也返回它）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteResult {
    pub operation_id: String,
    pub file_count: usize,
    pub span_count: usize,
}

/// 把某条歧义链接改写为指定的**完整相对路径**。
#[tauri::command]
pub async fn link_resolve_ambiguous(
    link_id: i64,
    target_rel_path: String,
    state: State<'_, AppState>,
) -> Result<RewriteResult, KpError> {
    let root = root_of(&state)?;
    let pool = state.index_db().ok_or(KpError(AppError::VaultNotOpen))?;

    let (src_rel, line, col, _old_target) = crate::storage::link_query::link_by_id(&pool, link_id)
        .map_err(KpError)?
        .ok_or_else(|| KpError(AppError::FileNotFound(format!("链接 id={link_id}"))))?;

    let target = root.join(&src_rel);
    let content =
        std::fs::read_to_string(&target).map_err(|e| KpError(AppError::io_at(&src_rel, &e)))?;

    // 位置口径与索引一致（1-based 行列）；对不上说明索引与磁盘已经不一致，如实报错而不是猜
    let (new_content, hits) = rewrite_occurrence_at(&content, line, col, &target_rel_path)
        .ok_or_else(|| {
            KpError(AppError::RewriteFailed(format!(
                "{src_rel} 的第 {line} 行第 {col} 列已不是那条链接（索引与磁盘可能不一致）"
            )))
        })?;

    let plan = RewritePlan {
        changes: vec![FileChange {
            rel_path: src_rel.clone(),
            old_content: content,
            new_content,
            hits,
        }],
    };
    let backup_root = root
        .join(kp_domain::vault_paths::INTERNAL_DIR)
        .join(kp_domain::vault_paths::BACKUP_DIR);
    let report = apply(&root, &plan, &backup_root)
        .map_err(|e| KpError(AppError::RewriteFailed(e.to_string())))?;

    let operation_id = next_id("op");
    put_operation(
        &state,
        operation_id.clone(),
        StoredOperation {
            backup_dir: report.backup_dir.clone(),
            vault_root: root.clone(),
            modified: vec![src_rel.clone()],
            renamed: None,
        },
    );
    tracing::info!(operation_id = %operation_id, rel_path = %src_rel, target = %target_rel_path, "歧义链接已改写为完整相对路径");

    Ok(RewriteResult {
        operation_id,
        file_count: report.files,
        span_count: report.hits,
    })
}
