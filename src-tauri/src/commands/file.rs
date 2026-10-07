//! 文件域只读命令（PRD §5.3.2；薄壳，RS-01）：实现见 kp_domain::file_tree。

use crate::commands::paths::root_of;
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::file_tree;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTreeArgs {
    /// 缺省为 Vault 根
    pub parent_rel_path: Option<String>,
    /// 缺省 false（AC-FILE-09：`.knowlpad/` 无论如何都隐藏）
    pub include_hidden: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileListDirArgs {
    pub rel_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatArgs {
    pub rel_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileValidateNameArgs {
    pub name: String,
}

/// 树节点（PRD §5.3.2.1）：**单层**返回，作为虚拟滚动数据源。
#[derive(Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    pub rel_path: String,
    pub name: String,
    pub is_dir: bool,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_children: Option<bool>,
}

impl From<file_tree::FileEntry> for FileNode {
    fn from(entry: file_tree::FileEntry) -> Self {
        Self {
            rel_path: entry.rel_path,
            name: entry.name,
            is_dir: entry.is_dir,
            kind: entry.kind.as_str().to_string(),
            size_bytes: entry.size_bytes,
            mtime_ms: entry.mtime_ms,
            has_children: entry.has_children,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    pub rel_path: String,
    pub is_dir: bool,
    pub kind: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 文件树（单层）。**禁止**返回全量树——虚拟滚动逐层展开（IPC-05 的 4MB 上限）。
#[tauri::command]
pub async fn file_tree(
    state: tauri::State<'_, AppState>,
    args: FileTreeArgs,
) -> Result<Vec<FileNode>, KpError> {
    let root = root_of(&state)?;
    let rel = args.parent_rel_path.unwrap_or_default();
    let include_hidden = args.include_hidden.unwrap_or(false);
    // DEBT-15 口径：**默认视图走索引库**（避免每次展开都递归 readdir）；
    // include_hidden = true 仍走直读 —— 索引按 IGNORED_DIRS 跳过了隐藏目录，
    // 而"显示隐藏项"的语义是"让我看见它们"，不能因为索引没收录就说它们不存在。
    if !include_hidden {
        if let Some(pool) = state.index_db() {
            if crate::storage::index_query::has_rows(&pool) {
                let entries =
                    crate::storage::index_query::list_children(&pool, &rel).map_err(KpError)?;
                return Ok(entries.into_iter().map(FileNode::from).collect());
            }
        }
    }
    let entries = tauri::async_runtime::spawn_blocking(move || {
        file_tree::list_dir(&root, &rel, include_hidden)
    })
    .await
    .map_err(|err| {
        tracing::error!(error = %err, "后台任务失败");
        KpError(AppError::IoFailure(
            "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
        ))
    })?
    .map_err(KpError)?;
    Ok(entries.into_iter().map(FileNode::from).collect())
}

/// 单层目录列表（懒加载）。与 `file_tree` 同一实现：**默认不显示隐藏项**。
#[tauri::command]
pub async fn file_list_dir(
    state: tauri::State<'_, AppState>,
    args: FileListDirArgs,
) -> Result<Vec<FileNode>, KpError> {
    let root = root_of(&state)?;
    let rel = args.rel_path;
    // DEBT-15 口径：本命令恒为「不显示隐藏项」，因此索引可用时优先走索引（PR-6）
    if let Some(pool) = state.index_db() {
        if crate::storage::index_query::has_rows(&pool) {
            let entries =
                crate::storage::index_query::list_children(&pool, &rel).map_err(KpError)?;
            return Ok(entries.into_iter().map(FileNode::from).collect());
        }
    }
    let entries =
        tauri::async_runtime::spawn_blocking(move || file_tree::list_dir(&root, &rel, false))
            .await
            .map_err(|err| {
                tracing::error!(error = %err, "后台任务失败");
                KpError(AppError::IoFailure(
                    "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
                ))
            })?
            .map_err(KpError)?;
    Ok(entries.into_iter().map(FileNode::from).collect())
}

/// 文件/目录元信息。
#[tauri::command]
pub async fn file_stat(
    state: tauri::State<'_, AppState>,
    args: FileStatArgs,
) -> Result<FileStat, KpError> {
    let root = root_of(&state)?;
    let rel = args.rel_path;
    let entry = tauri::async_runtime::spawn_blocking(move || file_tree::stat(&root, &rel))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "后台任务失败");
            KpError(AppError::IoFailure(
                "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
            ))
        })?
        .map_err(KpError)?;
    Ok(FileStat {
        rel_path: entry.rel_path,
        is_dir: entry.is_dir,
        kind: entry.kind.as_str().to_string(),
        size_bytes: entry.size_bytes.unwrap_or(0),
        mtime_ms: entry.mtime_ms.unwrap_or(0),
    })
}

/// 文件名合法性校验（FR-FILE-12）。纯计算，无需线程池。
#[tauri::command]
pub async fn file_validate_name(args: FileValidateNameArgs) -> Result<ValidationResult, KpError> {
    let result = file_tree::validate_name(&args.name);
    Ok(ValidationResult {
        valid: result.valid,
        reason: result.reason,
    })
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
