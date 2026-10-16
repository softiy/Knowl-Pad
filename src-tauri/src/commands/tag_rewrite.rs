//! 标签重命名的命令面（PRD **§5.3.6** 的 `tag_rename_preview` + **§5.3.3** 的 `tag_rename_apply`）。
//!
//! **复用链接改写的骨架**（这是范围裁定要求的做法）：
//! - 计划/落地/备份/整批回滚 = `kp_domain::link_rewrite_apply`（`RewritePlan` / `FileChange` / `apply` 本来就是通用形状）；
//! - 预览存储（TTL + 一次性消费）= `commands::link_rewrite_store`；
//! - 陈旧检测 = `commands::link_rewrite::ensure_fresh`（同一实现，两域共用）。
//!
//! **独立的只有 planner**：`kp_domain::tag_rewrite` —— 链接的 planner 是 `[[...]]` 专用的，不能套用到标签。
//!
//! AC-TAG-02 的两条硬要求都由骨架保证：① 改写前**备份全部文件**（`apply` 先全量备份再写）；
//! ② 任一文件失败 ⇒ **全部回滚且标签表保持原状**（`apply` 的全有或全无；标签表本身由索引重建，见报告"重索引口径"）。

use serde::Deserialize;
use tauri::State;

use crate::commands::link::pool_of;
use crate::commands::link_rewrite::{
    backup_root, ensure_fresh, log_rollback_failure, plan_to_edits, RewritePreview, RewriteResult,
};
use crate::commands::link_rewrite_store::{
    next_id, now_ms, put_operation, put_preview, take_preview, StoredOperation, StoredPreview,
};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use crate::storage::tag_query;
use kp_domain::error::AppError;
use kp_domain::link_rewrite_apply::{self, FileChange, RewritePlan};
use kp_domain::tag_rewrite;

/// `tag_rename_preview` 的入参（PRD §5.3.6.2 的 `TagRenamePreviewArgs`）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameArgs {
    /// 现存标签的**规范键**（`TagNode.norm`）。
    pub from_tag: String,
    /// 目标名称（允许大小写与 `/` 层级；非法嵌套/循环按 FR-TAG-08 拒绝）。
    pub to_tag: String,
}

/// `tag_rename_apply` 的入参（PRD §5.3.6.2 的 `TagRenameApplyArgs`）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameApplyArgs {
    pub from_tag: String,
    pub to_tag: String,
    /// 来自 `tag_rename_preview` 的返回，**一次性消费**。
    pub preview_id: String,
}

/// **预览**（只读，不写盘）：定位将改写的文件与处数并冻结。
pub(crate) fn tag_rename_preview_impl(
    state: &AppState,
    from_tag: String,
    to_tag: String,
) -> Result<RewritePreview, KpError> {
    let root = state
        .current_root()
        .ok_or(KpError(AppError::VaultNotOpen))?;
    let pool = pool_of(state)?;
    // FR-TAG-08：非法嵌套/循环在**预览阶段**就拒绝 ⇒ 磁盘零改动、也不产生 preview_id。
    tag_rewrite::validate_rename(&from_tag, &to_tag).map_err(KpError)?;

    let mut plan = RewritePlan::default();
    let mut mtimes: Vec<(String, i64)> = Vec::new();
    for rel_path in tag_query::files_with_tag(&pool, &from_tag).map_err(KpError)? {
        let abs = root.join(&rel_path);
        let old_content = match std::fs::read_to_string(&abs) {
            Ok(c) => c,
            // 审计 R3：不可读的文件必须**回报**（否则用户以为全改了，实际有一批原样留在磁盘上）
            Err(_) => {
                plan.skipped.push(rel_path.clone());
                continue;
            }
        };
        let outcome = tag_rewrite::rewrite_tags(&old_content, &from_tag, &to_tag);
        if outcome.hits == 0 {
            // 照链接改写口径：**没改的文件不进计划**（避免"打开了但没改"也产生备份噪音）
            continue;
        }
        mtimes.push((
            rel_path.clone(),
            kp_domain::note_io::mtime_ms(&abs).unwrap_or(0),
        ));
        plan.changes.push(FileChange {
            rel_path,
            old_content,
            new_content: outcome.new_content,
            hits: outcome.hits,
        });
    }

    let preview = RewritePreview {
        preview_id: next_id("tagpreview"),
        from_ref: from_tag.clone(),
        to_ref: to_tag.clone(),
        rename: None,
        file_count: plan.changes.len(),
        span_count: plan.total_hits(),
        edits: plan_to_edits(&plan),
        created_at_ms: now_ms(),
        skipped: plan.skipped.clone(),
    };
    put_preview(
        state,
        preview.preview_id.clone(),
        StoredPreview {
            created_at_ms: preview.created_at_ms,
            vault_root: root,
            from_ref: from_tag,
            to_ref: to_tag,
            rename: None,
            plan,
            mtimes,
        },
    );
    Ok(preview)
}

/// **落地**（⚠️ 写操作）：一次性消费 `preview_id`，全有或全无（AC-TAG-02）。
pub(crate) fn tag_rename_apply_impl(
    state: &AppState,
    from_tag: String,
    to_tag: String,
    preview_id: String,
) -> Result<RewriteResult, KpError> {
    // 一次性取出：未知/已消费/过期统一 E_PREVIEW_EXPIRED（PRD §5.2）。
    let stored = take_preview(state, &preview_id).ok_or(KpError(AppError::PreviewExpired))?;
    if stored.vault_root
        != state
            .current_root()
            .ok_or(KpError(AppError::VaultNotOpen))?
    {
        return Err(KpError(AppError::PreviewExpired));
    }
    // 预览里冻结的规格必须与执行时传的一致：否则用户改了输入再点确认，就会用**错的规格**改文件。
    if stored.from_ref != from_tag || stored.to_ref != to_tag {
        return Err(KpError(AppError::RewriteFailed(
            "预览与执行参数不一致，已中止（请重新预览）".to_string(),
        )));
    }
    ensure_fresh(&stored.vault_root, &stored.plan, &stored.mtimes).map_err(KpError)?;

    let operation_id = next_id("op");
    let report = link_rewrite_apply::apply(
        &stored.vault_root,
        &stored.plan,
        &backup_root(&stored.vault_root),
    )
    .map_err(|e| {
        log_rollback_failure(&e);
        KpError(e)
    })?;
    put_operation(
        state,
        operation_id.clone(),
        StoredOperation {
            backup_dir: report.backup_dir.clone(),
            vault_root: stored.vault_root.clone(),
            modified: stored
                .plan
                .changes
                .iter()
                .map(|c| c.rel_path.clone())
                .collect(),
            renamed: None,
        },
    );
    Ok(RewriteResult {
        operation_id,
        file_count: report.files,
        span_count: report.hits,
    })
}

// ── 2 个命令（薄壳：只做参数搬运与调用，CMD-01）──────────────────────────────

/// 标签重命名预览（PRD §5.3.6；FR-TAG-07/08）。
#[tauri::command]
pub async fn tag_rename_preview(
    state: State<'_, AppState>,
    args: TagRenameArgs,
) -> Result<RewritePreview, KpError> {
    tag_rename_preview_impl(&state, args.from_tag, args.to_tag)
}

/// 标签批量重命名（PRD §5.3.3 标注 ⚠️；FR-TAG-07）。
#[tauri::command]
pub async fn tag_rename_apply(
    state: State<'_, AppState>,
    args: TagRenameApplyArgs,
) -> Result<RewriteResult, KpError> {
    tag_rename_apply_impl(&state, args.from_tag, args.to_tag, args.preview_id)
}
