//! 索引路径下的文件树单层列表（commands::file 的 list_children 分支）。
//!
//! 抽出来有两个原因：① commands/file.rs 已贴近 CODE-11 的硬阈值；
//! ② 这段"索引结果 + 空目录补扫 + 合并"的逻辑此前没有独立可测的入口。

use std::path::Path;

use crate::error_wrapper::KpError;
use crate::storage::index_query;
use crate::storage::pool::DbPool;
use kp_domain::file_tree;
use kp_domain::file_tree::FileEntry;

/**
 * 用索引列出一层的直接子项，并**补一次单层目录扫描**把索引表达不了的空目录并进来。
 *
 * M2 复核 blocker：索引只记文件，空目录无法表达（新建文件夹后树里不出现）。
 *
 * 审计 R6：补扫失败**不能静默变空数组** —— 那会让上面那个 blocker 悄悄复发。
 * 这里仍降级（索引结果本身有效，不该整体失败），但两种失败都留痕。
 */
pub async fn list_children_via_index(
    pool: &DbPool,
    root: &Path,
    rel: &str,
) -> Result<Vec<FileEntry>, KpError> {
    let mut entries = index_query::list_children(pool, rel).map_err(KpError)?;
    let task = tauri::async_runtime::spawn_blocking({
        let root = root.to_path_buf();
        let rel = rel.to_string();
        move || file_tree::list_dir(&root, &rel, false)
    })
    .await;
    let scanned = match task {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => {
            tracing::warn!("目录补扫失败，空目录可能不显示");
            Vec::new()
        }
        Err(err) => {
            tracing::error!(error = %err, "目录补扫任务失败");
            Vec::new()
        }
    };
    index_query::merge_scanned_dirs(&mut entries, scanned);
    Ok(entries)
}
