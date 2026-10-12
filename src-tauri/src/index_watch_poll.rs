//! 轮询回退（inotify 耗尽等场景，NFR-PLAT-09）：每 5s 做一次「跳过未变更」的全量对账。
//!
//! 独立成文件的两个原因：① index_watch.rs 已逼近 CODE-11 的警告阈值（预算已满，超即失败）；
//! ② 「降级线程能否停下」本身值得有独立可读的实现（审计 T-21）。

use std::path::PathBuf;
use std::sync::Arc;

use crate::index_engine::{self, IndexMode};
use crate::index_watch::POLL_INTERVAL;
use crate::storage::pool::DbPool;

/// 轮询回退（inotify 耗尽等，NFR-PLAT-09）：每 5s 做一次「跳过未变更」的对账。
pub fn start_polling_fallback(
    pool: Arc<DbPool>,
    root: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    // 审计 T-21：循环必须能被停 —— 停止标志同时用作「睡眠后立即退出」与底层索引的取消谓词。
    let stop_for_cancel = stop.clone();
    std::thread::spawn(move || {
        while !stop.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(POLL_INTERVAL);
            if stop.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            match index_engine::full_index_cancellable(
                &pool,
                &root,
                |_, _| {},
                IndexMode::SkipUnchanged,
                || stop_for_cancel.load(std::sync::atomic::Ordering::SeqCst),
            ) {
                Ok(outcome) => {
                    if outcome.indexed > 0 {
                        tracing::info!(indexed = outcome.indexed, "轮询对账已索引");
                    }
                }
                Err(err) => tracing::warn!(error = %err, "轮询对账失败"),
            }
        }
    })
}
