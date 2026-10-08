//! 链接查询域命令（PRD §5.3.5 的**查询类**子集，6 个只读命令）。
//!
//! 不在本批：`link_resolve_ambiguous`（需链接改写器 + 备份/回滚，随 M4 改写 PR 落地）、
//! `graph_full`/`graph_local`（归 M6 图谱域）。
//!
//! 分工：SQL 在 `storage::link_query`，片段提取在 `kp_domain::snippet`（纯函数），
//! IPC 形状在 `commands::link_view`，本文件只做"读正文 → 组装 → 映射错误"。

use tauri::State;

use crate::commands::link_view::{
    read_note, snippet_of, AmbiguousItem, AmbiguousPage, BacklinkGroup, BacklinkItem, DanglingItem,
    DanglingPage, HeadingItem, OrphanPage, OutgoingLink,
};
use crate::commands::paths::root_of;
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;

fn pool_of(
    state: &State<'_, AppState>,
) -> Result<std::sync::Arc<crate::storage::pool::DbPool>, KpError> {
    state.index_db().ok_or(KpError(AppError::VaultNotOpen))
}

/// 反向链接（FR-LINK-10~15）：按来源分组，逐条带上下文片段。
///
/// `include_embeds = false` 时过滤掉 `embed` 类反链（面板默认分开显示两者）。
#[tauri::command]
pub async fn link_backlinks(
    rel_path: String,
    include_embeds: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<BacklinkGroup>, KpError> {
    let root = root_of(&state)?;
    let pool = pool_of(&state)?;
    let rows = crate::storage::link_query::backlinks_for(&pool, &rel_path).map_err(KpError)?;
    let include_embeds = include_embeds.unwrap_or(true);
    let mut cache: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    let mut groups: Vec<BacklinkGroup> = Vec::new();
    for row in rows {
        let is_embed = row.link_kind == "embed";
        if is_embed && !include_embeds {
            continue;
        }
        let content = cache
            .entry(row.src_rel_path.clone())
            .or_insert_with(|| read_note(&root, &row.src_rel_path));
        // [[...]] 的字符数：目标引用 + 4 个括号字符（高亮区间够用即可）
        let link_len = row.target_ref.chars().count() + 4;
        let snippet = content
            .as_deref()
            .and_then(|c| snippet_of(c, row.line, row.col, link_len));
        let item = BacklinkItem {
            line: row.line,
            col: row.col,
            link_kind: row.link_kind.clone(),
            alias: row.alias.clone(),
            anchor: row.anchor.clone(),
            snippet,
        };
        match groups
            .iter_mut()
            .find(|g| g.src_rel_path == row.src_rel_path)
        {
            Some(g) => {
                if is_embed {
                    g.embed_count += 1;
                } else {
                    g.link_count += 1;
                }
                g.items.push(item);
            }
            None => groups.push(BacklinkGroup {
                src_rel_path: row.src_rel_path.clone(),
                src_name: row.src_name.clone(),
                link_count: usize::from(!is_embed),
                embed_count: usize::from(is_embed),
                items: vec![item],
            }),
        }
    }
    Ok(groups)
}

/// 出链列表。
#[tauri::command]
pub async fn link_outgoing(
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<OutgoingLink>, KpError> {
    let pool = pool_of(&state)?;
    let rows = crate::storage::link_query::outgoing_for(&pool, &rel_path).map_err(KpError)?;
    Ok(rows
        .into_iter()
        .map(|r| OutgoingLink {
            target_ref: r.target_ref,
            status: r.status,
            anchor: r.anchor,
            alias: r.alias,
            link_kind: r.link_kind,
            line: r.line,
            col: r.col,
            dst_rel_path: r.dst_rel_path,
        })
        .collect())
}

/// 悬空链接清单（FR-LINK-20：按目标名分组 + 引用次数）。
#[tauri::command]
pub async fn link_dangling_list(state: State<'_, AppState>) -> Result<DanglingPage, KpError> {
    let pool = pool_of(&state)?;
    let items: Vec<DanglingItem> = crate::storage::link_query::dangling_groups(&pool)
        .map_err(KpError)?
        .into_iter()
        .map(|g| DanglingItem {
            target_ref: g.target_ref,
            ref_count: g.ref_count,
            source_count: g.source_count,
            sample_sources: g.sample_sources,
        })
        .collect();
    Ok(DanglingPage {
        total: items.len(),
        items,
    })
}

/// 歧义链接清单（FR-LINK-22 / AC-LINK-04）。
#[tauri::command]
pub async fn link_ambiguous_list(state: State<'_, AppState>) -> Result<AmbiguousPage, KpError> {
    let pool = pool_of(&state)?;
    let items: Vec<AmbiguousItem> = crate::storage::link_query::ambiguous_rows(&pool)
        .map_err(KpError)?
        .into_iter()
        .map(|r| AmbiguousItem {
            target_ref: r.target_ref,
            candidates: r.candidates,
            ref_count: r.ref_count,
        })
        .collect();
    Ok(AmbiguousPage {
        total: items.len(),
        items,
    })
}

/// 孤立笔记清单（FR-LINK-21）。
#[tauri::command]
pub async fn link_orphan_list(state: State<'_, AppState>) -> Result<OrphanPage, KpError> {
    let pool = pool_of(&state)?;
    let items = crate::storage::link_query::orphan_notes(&pool).map_err(KpError)?;
    Ok(OrphanPage {
        total: items.len(),
        items,
    })
}

/// 指定笔记的标题列表（供 FR-EDITOR-23 的标题补全）。
#[tauri::command]
pub async fn link_headings(
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<HeadingItem>, KpError> {
    let pool = pool_of(&state)?;
    let rows = crate::storage::link_query::headings_of(&pool, &rel_path).map_err(KpError)?;
    Ok(rows
        .into_iter()
        .map(|r| HeadingItem {
            level: r.level,
            text: r.text,
            anchor: r.anchor,
            line: r.line,
        })
        .collect())
}
