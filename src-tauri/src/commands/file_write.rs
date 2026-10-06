//! 文件域写操作命令（PRD §5.3.2；薄壳，RS-01）：实现见 `kp_domain::file_ops`。
//!
//! 契约要点：
//! - 目标已存在时必须由前端显式给出 `onConflict`（缺省按**最安全**的 `cancel` 处理，R-07 不静默覆盖）；
//! - `file_delete` 是**软删除**（移入回收站），不做永久删除（FR-FILE-30）；
//! - 覆盖既有文件时域层会先备份，备份位置在此记录审计日志（SEC-09：只记相对路径）。

use crate::commands::note::WriteResult;
use crate::commands::paths::root_of;
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::file_ops::{self, ConflictPolicy};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCreateArgs {
    pub rel_path: String,
    /// 缺省为空文档
    pub content: Option<String>,
    /// overwrite / renameNew / cancel（缺省 cancel）
    pub on_conflict: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderCreateArgs {
    pub rel_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRenameArgs {
    pub from: String,
    pub to: String,
    pub on_conflict: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeleteArgs {
    pub rel_path: String,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRevealArgs {
    pub rel_path: String,
}

/// 重命名 / 移动结果（PRD §5.3.2.1 的 RenameResult）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameResult {
    pub from: String,
    pub to: String,
    pub new_mtime_ms: i64,
}

/// 删除结果（PRD §5.3.2.1 的 DeleteResult）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub rel_path: String,
    pub trashed_count: u64,
}

fn policy_of(value: Option<String>) -> Result<ConflictPolicy, KpError> {
    match value.as_deref() {
        None => Ok(ConflictPolicy::Cancel),
        Some(text) => ConflictPolicy::parse(text).map_err(KpError),
    }
}

fn join_error(err: tauri::Error) -> KpError {
    {
        tracing::error!(error = %err, "后台任务失败");
        KpError(AppError::IoFailure(
            "操作未能完成，后台任务异常。请重试；若持续出现，请重启应用。".to_string(),
        ))
    }
}

/// 新建笔记（FR-FILE-10/13）。
#[tauri::command]
pub async fn note_create(
    state: tauri::State<'_, AppState>,
    args: NoteCreateArgs,
) -> Result<WriteResult, KpError> {
    let root = root_of(&state)?;
    let policy = policy_of(args.on_conflict)?;
    let content = args.content.unwrap_or_default();
    let rel_path = args.rel_path;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        file_ops::create_note(&root, &rel_path, &content, policy)
    })
    .await
    .map_err(join_error)?
    .map_err(KpError)?;
    if let Some(backup) = &outcome.backed_up_to {
        tracing::info!(backed_up_to = %backup, "覆盖前已备份原文件");
    }
    Ok(WriteResult {
        rel_path: outcome.rel_path,
        new_mtime: outcome.new_mtime_ms,
    })
}

/// 新建文件夹（FR-FILE-11：支持多级）。
#[tauri::command]
pub async fn folder_create(
    state: tauri::State<'_, AppState>,
    args: FolderCreateArgs,
) -> Result<(), KpError> {
    let root = root_of(&state)?;
    let rel_path = args.rel_path;
    tauri::async_runtime::spawn_blocking(move || file_ops::create_folder(&root, &rel_path))
        .await
        .map_err(join_error)?
        .map_err(KpError)?;
    Ok(())
}

async fn rename_impl(
    state: tauri::State<'_, AppState>,
    args: FileRenameArgs,
) -> Result<RenameResult, KpError> {
    let root = root_of(&state)?;
    let policy = policy_of(args.on_conflict)?;
    let (from, to) = (args.from, args.to);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        file_ops::rename_path(&root, &from, &to, policy)
    })
    .await
    .map_err(join_error)?
    .map_err(KpError)?;
    if let Some(backup) = &outcome.backed_up_to {
        tracing::info!(backed_up_to = %backup, "覆盖前已备份原文件");
    }
    Ok(RenameResult {
        from: outcome.from,
        to: outcome.to,
        new_mtime_ms: outcome.new_mtime_ms,
    })
}

/// 重命名（**不含链接改写**，改写属 M4；FR-FILE-28 的存储侧）。
#[tauri::command]
pub async fn file_rename(
    state: tauri::State<'_, AppState>,
    args: FileRenameArgs,
) -> Result<RenameResult, KpError> {
    rename_impl(state, args).await
}

/// 移动（与重命名同一实现；跨目录即移动）。
#[tauri::command]
pub async fn file_move(
    state: tauri::State<'_, AppState>,
    args: FileRenameArgs,
) -> Result<RenameResult, KpError> {
    rename_impl(state, args).await
}

/// 删除 = 软删除，移入回收站（FR-FILE-30；完整回收站流程属 M7）。
#[tauri::command]
pub async fn file_delete(
    state: tauri::State<'_, AppState>,
    args: FileDeleteArgs,
) -> Result<DeleteResult, KpError> {
    let root = root_of(&state)?;
    let (rel_path, recursive) = (args.rel_path, args.recursive);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        file_ops::delete_path(&root, &rel_path, recursive)
    })
    .await
    .map_err(join_error)?
    .map_err(KpError)?;
    tracing::info!(
        rel_path = %outcome.rel_path,
        trash_rel_path = %outcome.trash_rel_path,
        "已移入回收站"
    );
    Ok(DeleteResult {
        rel_path: outcome.rel_path,
        trashed_count: outcome.trashed_count,
    })
}

/// 在系统文件管理器中显示（技术方案 §3.2：opener 仅 Rust 侧调用）。
#[tauri::command]
pub async fn file_reveal(
    state: tauri::State<'_, AppState>,
    args: FileRevealArgs,
) -> Result<(), KpError> {
    let root = root_of(&state)?;
    let rel_path = args.rel_path;
    let abs = tauri::async_runtime::spawn_blocking(move || {
        let guard = kp_domain::path_guard::PathGuard::new(&root)?;
        guard.resolve(&rel_path)
    })
    .await
    .map_err(join_error)?
    .map_err(KpError)?;
    tauri_plugin_opener::reveal_item_in_dir(&abs).map_err(|err| {
        tracing::warn!(error = %err, "在文件管理器中显示失败");
        KpError(AppError::IoFailure(format!(
            "无法在文件管理器中显示：{err}"
        )))
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "file_write_tests.rs"]
mod tests;
