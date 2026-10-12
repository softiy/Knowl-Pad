//! 链接改写域命令（PRD §5.3.3；4 个里本批 3 个，\`tag_rename_apply\` 属标签域 → M5）。
//! **两阶段设计**（技术方案 §6.1.1，PRD §5.3.3 明文）：

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::link_rewrite_store::{
    next_id, now_ms, put_operation, put_preview, take_preview,
};
use crate::commands::link_rewrite_store::{StoredOperation, StoredPreview};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::link_rewrite_apply::{apply, plan_rename, rename_with_rewrite};

/// 改名规格（PRD §5.3.3 的 \`rename: { from, to }\`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSpec {
    pub from: String,
    pub to: String,
}

/// 一处改写的前后片段（如 `[[A]]` → `[[B]]`）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceDiffView {
    pub line: u32,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteEdit {
    pub rel_path: String,
    pub hits: usize,
    /// 逐处差异（技术方案 §6.1.1：每一处的原文 → 新文）。为控 IPC 载荷，每个文件最多 20 处；
    /// 完整处数仍看 `hits`。
    pub occurrences: Vec<OccurrenceDiffView>,
}

/// PRD §5.3.3 的 \`RewritePreview\`：\`preview_id\` + 明细（文件数、处数、逐文件处数）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewritePreview {
    pub preview_id: String,
    pub from_ref: String,
    pub to_ref: String,
    /// 冻结的改名规格（有则执行阶段与之合并为同一次可回滚操作，FR-FILE-22）。
    pub rename: Option<RenameSpec>,
    pub file_count: usize,
    pub span_count: usize,
    pub edits: Vec<RewriteEdit>,
    pub created_at_ms: u128,
}

/// PRD §5.3.3 的 \`RewriteResult\`。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteResult {
    pub operation_id: String,
    pub file_count: usize,
    pub span_count: usize,
}

fn pool_of(state: &AppState) -> Result<std::sync::Arc<crate::storage::pool::DbPool>, KpError> {
    state.index_db().ok_or(KpError(AppError::VaultNotOpen))
}

fn backup_root(root: &std::path::Path) -> std::path::PathBuf {
    root.join(kp_domain::vault_paths::INTERNAL_DIR)
        .join(kp_domain::vault_paths::BACKUP_DIR)
}

fn candidates(pool: &crate::storage::pool::DbPool, from_ref: &str) -> Result<Vec<String>, KpError> {
    let stem = kp_domain::link_rewrite::stem_of(from_ref);
    crate::storage::link_query::files_referencing(pool, &stem).map_err(KpError)
}

/// **预览**（只读）：定位将改写的处数并冻结，返回 \`preview_id\`（FR-FILE-21 ②）。
/// 命令体（把状态作为参数传入，便于测试 —— 项目既有做法，见 DEBT-14）。
/// 把改写计划映射成预览用的逐文件条目（含逐处差异）。
fn plan_to_edits(plan: &kp_domain::link_rewrite_apply::RewritePlan) -> Vec<RewriteEdit> {
    /// 每个文件最多展示多少处差异（IPC 载荷控制）。
    const MAX_PER_FILE: usize = 20;
    plan.changes
        .iter()
        .map(|c| RewriteEdit {
            rel_path: c.rel_path.clone(),
            hits: c.hits,
            occurrences: kp_domain::link_rewrite_diff::occurrence_diffs(
                &c.old_content,
                &c.new_content,
                MAX_PER_FILE,
            )
            .into_iter()
            .map(|d| OccurrenceDiffView {
                line: d.line,
                before: d.before,
                after: d.after,
            })
            .collect(),
        })
        .collect()
}

pub(crate) fn link_rewrite_preview_impl(
    state: &AppState,
    from_ref: String,
    to_ref: String,
    rename: Option<RenameSpec>,
) -> Result<RewritePreview, KpError> {
    let root = state
        .current_root()
        .ok_or(KpError(AppError::VaultNotOpen))?;
    let pool = pool_of(state)?;
    let cands = candidates(&pool, &from_ref)?;
    let plan = plan_rename(&root, &from_ref, &to_ref, &cands).map_err(KpError)?;

    let preview = RewritePreview {
        preview_id: next_id("p"),
        from_ref: from_ref.clone(),
        to_ref: to_ref.clone(),
        rename: rename.clone(),
        file_count: plan.changes.len(),
        span_count: plan.total_hits(),
        edits: plan_to_edits(&plan),
        created_at_ms: now_ms(),
    };
    put_preview(
        state,
        preview.preview_id.clone(),
        StoredPreview {
            created_at_ms: preview.created_at_ms,
            // 冻结每个待改文件的 mtime（陈旧检测的一半；另一半是内容比对，见 apply）
            mtimes: plan
                .changes
                .iter()
                .map(|c| {
                    (
                        c.rel_path.clone(),
                        kp_domain::note_io::mtime_ms(&root.join(&c.rel_path)).unwrap_or(0),
                    )
                })
                .collect(),
            vault_root: root,
            from_ref,
            to_ref,
            rename: rename.map(|r| (r.from, r.to)),
            plan,
        },
    );
    Ok(preview)
}

/// 命令体（把状态作为参数传入，便于测试 —— 项目既有做法，见 DEBT-14）。
pub(crate) fn link_rewrite_apply_impl(
    state: &AppState,
    preview_id: String,
    rename: Option<RenameSpec>,
) -> Result<RewriteResult, KpError> {
    // 一次性取出：取不到（未知/已消费/过期）统一是 E_PREVIEW_EXPIRED（PRD §5.2）。
    let stored = take_preview(state, &preview_id).ok_or(KpError(AppError::PreviewExpired))?;
    if stored.vault_root
        != state
            .current_root()
            .ok_or(KpError(AppError::VaultNotOpen))?
    {
        return Err(KpError(AppError::PreviewExpired));
    }
    let rename_spec = rename
        .map(|r| (r.from, r.to))
        .or_else(|| stored.rename.clone());

    // ── 陈旧检测（技术方案 §6.1.5，双重保护：mtime + 内容）──────────────────────
    // 预览冻结了每个待改文件的 (mtime, 原文)。预览到执行之间用户可能切到别的编辑器改了
    // 文件 —— 那时按旧内容整文件覆盖就是**静默覆盖用户数据**（P-2 / R-07 禁止）。
    // 任一文件不一致 → 整体中止，**磁盘零改动**（技术方案称这是最危险的失败模式）。
    let mut stale: Vec<String> = Vec::new();
    for change in &stored.plan.changes {
        let abs = stored.vault_root.join(&change.rel_path);
        let expect_mtime = stored
            .mtimes
            .iter()
            .find(|(rel, _)| rel == &change.rel_path)
            .map(|(_, m)| *m);
        let changed = match expect_mtime {
            // ① mtime 变了 → 陈旧
            Some(m) if m != kp_domain::note_io::mtime_ms(&abs).unwrap_or(0) => true,
            // ② mtime 没变（或没记录）时比内容；读不到（被删/权限变）也算陈旧
            _ => std::fs::read_to_string(&abs)
                .map(|now| now != change.old_content)
                .unwrap_or(true),
        };
        if changed {
            stale.push(change.rel_path.clone());
        }
    }
    if !stale.is_empty() {
        return Err(KpError(AppError::RewriteFailed(format!(
            "预览之后这些文件已被外部修改，已中止且未改动任何文件：{}",
            stale.join("、")
        ))));
    }

    let operation_id = next_id("op");
    let (report, renamed) = match &rename_spec {
        Some((from, to)) => {
            // 重命名与改写同属一次可回滚操作（FR-FILE-22）
            let (report, _new_rel) = rename_with_rewrite(
                &stored.vault_root,
                from,
                to,
                &stored
                    .plan
                    .changes
                    .iter()
                    .map(|c| c.rel_path.clone())
                    .collect::<Vec<_>>(),
                &backup_root(&stored.vault_root),
            )
            .map_err(|e| KpError(AppError::RewriteFailed(e.to_string())))?;
            (report, Some((from.clone(), to.clone())))
        }
        None => {
            let report = apply(
                &stored.vault_root,
                &stored.plan,
                &backup_root(&stored.vault_root),
            )
            .map_err(|e| KpError(AppError::RewriteFailed(e.to_string())))?;
            (report, None)
        }
    };

    // from_ref/to_ref 用于可追溯的日志（PRD ERR-04：写操作必须留痕）
    tracing::info!(
        operation_id = %operation_id,
        from_ref = %stored.from_ref,
        to_ref = %stored.to_ref,
        files = report.files,
        spans = report.hits,
        renamed = rename_spec.is_some(),
        "改写已落地（备份保留，可用 link_rewrite_rollback 回滚）"
    );

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
            renamed,
        },
    );

    Ok(RewriteResult {
        operation_id,
        file_count: report.files,
        span_count: report.hits,
    })
}

#[tauri::command]
pub async fn link_rewrite_preview(
    from_ref: String,
    to_ref: String,
    rename: Option<RenameSpec>,
    state: State<'_, AppState>,
) -> Result<RewritePreview, KpError> {
    link_rewrite_preview_impl(&state, from_ref, to_ref, rename)
}

#[tauri::command]
pub async fn link_rewrite_apply(
    preview_id: String,
    rename: Option<RenameSpec>,
    state: State<'_, AppState>,
) -> Result<RewriteResult, KpError> {
    link_rewrite_apply_impl(&state, preview_id, rename)
}
