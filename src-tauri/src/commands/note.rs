use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::note_io;
use kp_domain::path_guard::PathGuard;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteReadArgs {
    pub rel_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteWriteArgs {
    pub rel_path: String,
    pub content: String,
    pub base_mtime: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub rel_path: String,
    pub new_mtime: i64,
}

/// 取当前 Vault 根；未打开时返回 E_VAULT_NOT_OPEN。
fn root_of(state: &tauri::State<'_, AppState>) -> Result<PathBuf, KpError> {
    state
        .current_root()
        .ok_or(KpError(kp_domain::error::AppError::VaultNotOpen))
}

/// 在给定根下做七步路径校验（R-08：IO 与校验都不在 IPC 线程执行）。
fn resolve_in(root: &Path, rel_path: &str) -> Result<PathBuf, KpError> {
    let guard = PathGuard::new(root).map_err(KpError)?;
    guard.resolve(rel_path).map_err(KpError)
}

fn join_error(err: tauri::Error) -> KpError {
    KpError(kp_domain::error::AppError::IoFailure(format!(
        "后台任务失败：{err}"
    )))
}

/// 读取笔记：异步 command + spawn_blocking，避免阻塞 IPC 线程（R-08）。
#[tauri::command]
pub async fn note_read(
    state: tauri::State<'_, AppState>,
    args: NoteReadArgs,
) -> Result<String, KpError> {
    let root = root_of(&state)?;
    let rel_path = args.rel_path;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_in(&root, &rel_path)?;
        let bytes = note_io::read_note(&path)?;
        String::from_utf8(bytes)
            .map_err(|err| KpError(kp_domain::error::AppError::IoFailure(err.to_string())))
    });
    task.await.map_err(join_error)?
}

/// 写入笔记：冲突检测与原子写入均在线程池执行，IPC 线程只做参数搬运。
#[tauri::command]
pub async fn note_write(
    state: tauri::State<'_, AppState>,
    args: NoteWriteArgs,
) -> Result<WriteResult, KpError> {
    let root = root_of(&state)?;
    let NoteWriteArgs {
        rel_path,
        content,
        base_mtime,
    } = args;
    let result_path = rel_path.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_in(&root, &rel_path)?;
        let new_mtime = note_io::write_note(&path, content.as_bytes(), base_mtime)?;
        Ok::<i64, KpError>(new_mtime)
    });
    let new_mtime = task.await.map_err(join_error)??;
    Ok(WriteResult {
        rel_path: result_path,
        new_mtime,
    })
}
