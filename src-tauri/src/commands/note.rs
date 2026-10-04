use crate::commands::paths::{resolve_in, root_of};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::note_io;
use serde::{Deserialize, Serialize};

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

/// 笔记内容（PRD §5.3.2.1）：正文 + 冲突检测基线。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteContent {
    pub rel_path: String,
    pub content: String,
    /// 作为 note_write 的 baseMtime（FR-EDITOR-34 冲突检测）
    pub mtime_ms: i64,
    pub size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub rel_path: String,
    pub new_mtime: i64,
}

fn join_error(err: tauri::Error) -> KpError {
    KpError(kp_domain::error::AppError::IoFailure(format!(
        "后台任务失败：{err}"
    )))
}

/// 读取笔记：返回 `NoteContent`（含 `mtimeMs`，编辑器据此做冲突检测）。异步 command + spawn_blocking（R-08）。
#[tauri::command]
pub async fn note_read(
    state: tauri::State<'_, AppState>,
    args: NoteReadArgs,
) -> Result<NoteContent, KpError> {
    let root = root_of(&state)?;
    let rel_path = args.rel_path;
    let rel_for_result = rel_path.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_in(&root, &rel_path)?;
        let bytes = note_io::read_note(&path)?;
        let content = String::from_utf8(bytes)
            .map_err(|err| KpError(kp_domain::error::AppError::IoFailure(err.to_string())))?;
        let mtime_ms = note_io::mtime_ms(&path).map_err(KpError)?;
        let size_bytes = std::fs::metadata(&path)
            .map_err(|err| KpError(kp_domain::error::AppError::from(err)))?
            .len();
        Ok::<NoteContent, KpError>(NoteContent {
            rel_path: rel_for_result,
            content,
            mtime_ms,
            size_bytes,
        })
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
