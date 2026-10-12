//! 链接的**只读列表**命令（悬空链接；审计 L1 的顺带抽取）。
//!
//! 为什么单独一个文件：`link.rs` 已逼近 CODE-11 的 200 行阈值（为 L1 加一行就会破），
//! 而项目对"行数"的处置一贯是**抽取**而不是压缩注释。这里搬来最独立的一组
//! （悬空链接列表），命令体仍是 `_impl` + `#[tauri::command]` 薄包装（DEBT-14）。

use tauri::State;

use crate::commands::link::pool_of;
use crate::commands::link_view::{DanglingItem, DanglingPage};
use crate::error_wrapper::KpError;
use crate::state::AppState;

/// 悬空链接清单（FR-LINK-20：按目标名分组 + 引用次数）。
pub(crate) fn link_dangling_list_impl(state: &AppState) -> Result<DanglingPage, KpError> {
    let pool = pool_of(state)?;
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
#[tauri::command]
pub async fn link_dangling_list(state: State<'_, AppState>) -> Result<DanglingPage, KpError> {
    link_dangling_list_impl(&state)
}
