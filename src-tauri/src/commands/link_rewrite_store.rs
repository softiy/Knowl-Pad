//! 改写预览缓存的读写的**薄封装**（技术方案 §6.1.1 的两阶段设计）。
//!
//! 预览与执行分离的意义：预览阶段定位到的 span 被**冻结**，执行阶段直接复用、不重新扫描，
//! 从而避免"预览时看到 N 处、执行时改了 M 处"的不一致；TTL 防止缓存无限增长。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::error_wrapper::KpError;
use std::path::PathBuf;

use crate::state::AppState;
use kp_domain::error::AppError;

/// 预览有效期（技术方案 §6.1.1：TTL 10 分钟）。
pub const PREVIEW_TTL_MS: u128 = 10 * 60 * 1000;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 生成单调递增的 id。
///
/// 技术方案原本用 uuid/dashmap，但两者**都未引入**（§8 依赖表注明"需要时再评估"）；
/// 这里用"毫秒时间戳 + 进程内自增序号"，无需新依赖且天然单调、便于日志排查。
pub fn next_id(prefix: &str) -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}{ms}-{seq}")
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// 放入预览缓存。
pub fn put_preview(state: &AppState, id: String, preview: StoredPreview) {
    if let Ok(mut guard) = state.previews_lock().lock() {
        guard.insert(id, preview);
    }
}

/// **一次性取出**预览（执行阶段）：取走即从缓存移除，过期则丢弃并返回 None。
pub fn take_preview(state: &AppState, id: &str) -> Option<StoredPreview> {
    let taken = state
        .previews_lock()
        .lock()
        .ok()
        .and_then(|mut guard| guard.remove(id))?;
    if now_ms().saturating_sub(taken.created_at_ms) > PREVIEW_TTL_MS {
        return None; // 过期：不返回，也不放回（等价于"已消费"）
    }
    Some(taken)
}

/// 登记已执行的操作，供回滚。
pub fn put_operation(state: &AppState, id: String, op: StoredOperation) {
    if let Ok(mut guard) = state.operations_lock().lock() {
        guard.insert(id, op);
    }
}

pub fn get_operation(state: &AppState, id: &str) -> Option<StoredOperation> {
    state
        .operations_lock()
        .lock()
        .ok()
        .and_then(|g| g.get(id).cloned())
}

/// **回滚**：从备份恢复该操作改过的文件；若该操作含重命名，**先**恢复链接、**再**把文件改回原名
/// （技术方案 §6.1.3 步骤 4 明文要求这个顺序）。
/// 命令体（把状态作为参数传入，便于测试 —— 项目既有做法，见 DEBT-14）。
pub(crate) fn link_rewrite_rollback_impl(
    state: &AppState,
    operation_id: String,
) -> Result<(), KpError> {
    let op = get_operation(state, &operation_id)
        .ok_or_else(|| KpError(AppError::FileNotFound(operation_id.clone())))?;

    for (i, rel) in op.modified.iter().enumerate() {
        let backup = op.backup_dir.join(format!("{i}.bak"));
        let Ok(content) = std::fs::read(&backup) else {
            continue; // 没有这份备份（或已被清理）→ 跳过，不阻断其余恢复
        };
        let target = op.vault_root.join(rel);
        kp_domain::note_io::atomic_write(&target, &content)
            .map_err(|e| KpError(AppError::RewriteFailed(format!("回滚 {rel} 失败：{e}"))))?;
    }

    // 链接已改回旧名 → 现在把文件也改回旧名
    if let Some((from, to)) = &op.renamed {
        let to_abs = op.vault_root.join(to);
        let from_abs = op.vault_root.join(from);
        if to_abs.exists() {
            std::fs::rename(&to_abs, &from_abs).map_err(|e| {
                KpError(AppError::RewriteFailed(format!(
                    "链接已回滚，但文件改回原名失败（{from}）：{e}"
                )))
            })?;
        }
    }
    Ok(())
}

/// 预览缓存里的一条（预览阶段的产出，执行阶段消费）。
#[derive(Debug, Clone)]
pub struct StoredPreview {
    /// 单调递增的毫秒时间戳，用于 TTL 判定（技术方案：10 分钟）。
    pub created_at_ms: u128,
    pub vault_root: PathBuf,
    pub from_ref: String,
    pub to_ref: String,
    /// 由重命名/移动触发时**冻结**的改名规格，执行阶段纳入同一可回滚操作（FR-FILE-22）。
    pub rename: Option<(String, String)>,
    pub plan: kp_domain::link_rewrite_apply::RewritePlan,
}

/// 已执行操作的一条登记（回滚用）。
#[derive(Debug, Clone)]
pub struct StoredOperation {
    pub backup_dir: PathBuf,
    pub vault_root: PathBuf,
    /// 被改写的文件（相对路径），回滚时按它恢复。
    pub modified: Vec<String>,
    /// 该操作顺带做过的重命名（先恢复链接、再把文件改回原名，见技术方案 §6.1.3 步骤 4）。
    pub renamed: Option<(String, String)>,
}

#[tauri::command]
pub async fn link_rewrite_rollback(
    operation_id: String,
    state: State<'_, AppState>,
) -> Result<(), KpError> {
    link_rewrite_rollback_impl(&state, operation_id)
}
