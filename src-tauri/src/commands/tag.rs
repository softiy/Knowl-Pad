//! 标签域命令（PRD §5.3.6 的 **5 个命令**中的 2 个只读命令；名字与入参以该表为准）。
//!
//! **本批（W1+W2）只实现只读的两条**：`tag_tree` / `tag_notes`。
//! `tag_suggest` 与 `tag_delete_apply` **本轮不实现**（契约保留、缺位登记在 PRD 勘误 **D-38**）：
//! 前者唯一消费者 FR-TAG-06 随 M8，后者是 P2。
//! 写操作（`tag_rename_preview` / `tag_rename_apply`）属后续 PR-B，**不在本文件**。
//!
//! 结构遵循本仓既有分层：命令体（`*_impl`，可直接用真实临时库单测 —— DEBT-14 的做法）在这里，
//! SQL 在 `storage::tag_query`，树的构建在 `kp_domain::tag_tree`，线格式在 `tag_view.rs`。

use serde::Deserialize;
use tauri::State;

use crate::commands::link::pool_of;
use crate::commands::tag_view::{TagNode, TaggedNote};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use crate::storage::tag_query;

/// `tag_notes` 的入参（PRD §5.3.6.2 的 `TagNotesArgs`）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagNotesArgs {
    /// 小写规范键；**空串 = 「未分类」查询**（列出无任何标签的笔记）。
    pub tag_norm: String,
    /// 是否含全部子孙（缺省 `false`；FR-TAG-03「可切换是否含子级」）。
    #[serde(default)]
    pub include_children: bool,
}

/// 标签树（含计数与「未分类」条目）。
///
/// 「未分类」由 `kp_domain::tag_tree::unclassified_node` 合成，**排在全部标签之后**（PRD §5.3.6.1 的决策说明）。
pub(crate) fn tag_tree_impl(state: &AppState) -> Result<Vec<TagNode>, KpError> {
    let pool = pool_of(state)?;
    let rows = tag_query::load_tag_rows(&pool).map_err(KpError)?;
    let mut tree: Vec<TagNode> = kp_domain::tag_tree::build_tree(&rows)
        .into_iter()
        .map(Into::into)
        .collect();
    let unclassified = tag_query::unclassified_count(&pool).map_err(KpError)?;
    tree.push(kp_domain::tag_tree::unclassified_node(unclassified).into());
    Ok(tree)
}

/// 某标签下的笔记（FR-TAG-03）。
pub(crate) fn tag_notes_impl(
    state: &AppState,
    args: TagNotesArgs,
) -> Result<Vec<TaggedNote>, KpError> {
    let pool = pool_of(state)?;
    let rows =
        tag_query::notes_of_tag(&pool, &args.tag_norm, args.include_children).map_err(KpError)?;
    Ok(rows.into_iter().map(Into::into).collect())
}

// ── 2 个命令（薄壳：只做参数搬运与调用，CMD-01）──────────────────────────────

/// 标签树（PRD §5.3.6；FR-TAG-01/02/13）。
#[tauri::command]
pub async fn tag_tree(state: State<'_, AppState>) -> Result<Vec<TagNode>, KpError> {
    tag_tree_impl(&state)
}

/// 某标签下的笔记（PRD §5.3.6；FR-TAG-03）。
#[tauri::command]
pub async fn tag_notes(
    state: State<'_, AppState>,
    args: TagNotesArgs,
) -> Result<Vec<TaggedNote>, KpError> {
    tag_notes_impl(&state, args)
}
