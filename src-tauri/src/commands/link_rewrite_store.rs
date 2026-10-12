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
    // 审计 R8：此前锁中毒时**静默不插入** —— 于是"预览成功"却永远执行不了，
    // 而用户看到的只是"预览已过期"。写入路径一律留痕（读路径同理，见下）。
    match state.previews_lock().lock() {
        Ok(mut guard) => {
            guard.insert(id, preview);
        }
        Err(_) => tracing::error!(
            kind = "LockPoisoned",
            "改写预览登记失败：状态锁已中毒（该预览将无法执行）"
        ),
    }
}

/// **一次性取出**预览（执行阶段）：取走即从缓存移除，过期则丢弃并返回 None。
pub fn take_preview(state: &AppState, id: &str) -> Option<StoredPreview> {
    let taken = match state.previews_lock().lock() {
        Ok(mut guard) => guard.remove(id),
        Err(_) => {
            tracing::error!(kind = "LockPoisoned", "读取改写预览失败：状态锁已中毒");
            None
        }
    }?;
    if now_ms().saturating_sub(taken.created_at_ms) > PREVIEW_TTL_MS {
        return None; // 过期：不返回，也不放回（等价于"已消费"）
    }
    Some(taken)
}

/// 登记已执行的操作，供回滚。
pub fn put_operation(state: &AppState, id: String, op: StoredOperation) {
    // 审计 R8：这条最要紧 —— 登记丢失意味着**这次改写再也无法回滚**（备份还在，但没人知道它对应谁）。
    match state.operations_lock().lock() {
        Ok(mut guard) => {
            guard.insert(id, op);
        }
        Err(_) => tracing::error!(
            kind = "LockPoisoned",
            "回滚登记失败：状态锁已中毒（本次改写将无法通过 UI 回滚；备份仍在 .knowlpad 下）"
        ),
    }
}

pub fn get_operation(state: &AppState, id: &str) -> Option<StoredOperation> {
    match state.operations_lock().lock() {
        Ok(g) => g.get(id).cloned(),
        Err(_) => {
            tracing::error!(kind = "LockPoisoned", "读取回滚登记失败：状态锁已中毒");
            None
        }
    }
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

    let mut missing: Vec<String> = Vec::new();
    for (i, rel) in op.modified.iter().enumerate() {
        let backup = op.backup_dir.join(format!("{i}.bak"));
        let Ok(content) = std::fs::read(&backup) else {
            // **缺备份不是"跳过"**（审计 R2）：该文件仍停在改写后的内容，而 UI 会以为已还原、
            // 连回滚按钮都收起来。必须点名报错，让用户知道哪些文件没能恢复。
            missing.push(rel.clone());
            continue;
        };
        let target = op.vault_root.join(rel);
        kp_domain::note_io::atomic_write(&target, &content)
            .map_err(|e| KpError(AppError::RewriteFailed(format!("回滚 {rel} 失败：{e}"))))?;
    }

    if !missing.is_empty() {
        return Err(KpError(AppError::RollbackFailed {
            failures: missing,
            backup_dir: op.backup_dir.display().to_string(),
        }));
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
    /// **陈旧检测（技术方案 §6.1.5）**：预览时逐文件冻结的 (相对路径, mtime 毫秒)。
    /// 预览到执行之间用户可能切到别的编辑器改了文件 —— 那时按旧内容整文件覆盖就是静默丢数据。
    pub mtimes: Vec<(String, i64)>,
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
