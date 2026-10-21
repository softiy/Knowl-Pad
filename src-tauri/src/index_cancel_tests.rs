//! **DEBT-45 的回归用例**：取消 ≠ 完成。
//!
//! 背景（技术方案 §13.2 `DEBT-45`）：`full_index_cancellable` 按批 COMMIT（BATCH_SIZE=200），取消在批间发生 ⇒
//! 已提交的批次留在库里；修复前 `index_rebuild` 在 `Ok(cancelled=true)` 时仍 `set_index_ready(true)` 并 emit
//! `kp://index/completed` ⇒ 应用把"半索引"当完整索引信任。修复后：取消 ⇒ 不就绪 + 写 `rebuild_in_progress`
//! 标记（`ensure_index_db` 见到它就判 `Rebuilt`，见 `index_tests::interrupted_rebuild_is_restarted` 的对偶）+
//! 只发 `kp://index/cancelled`。

use std::fs;
use std::sync::Mutex;

use crate::commands::index::{ending_event, settle_rebuild, RebuildEnding};
use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::state::AppState;
use crate::storage::index::{open, read_meta, META_REBUILD_IN_PROGRESS};
use crate::storage::pool::DbPool;

fn build_vault(n: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..n {
        let path = root.join(format!("note-{i}.md"));
        fs::write(&path, format!("# 笔记 {i}\n\n内容 [[note-0]] #取消/测试\n")).expect("写笔记");
    }
    (dir, root)
}

fn pool() -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("索引库临时目录");
    let pool = open(&dir.path().join("index.db")).expect("打开索引库");
    (dir, pool)
}

/// 取消 ⇒ **不就绪**、留下不可信标记、且收尾判定是 Cancelled。
#[test]
fn cancelled_rebuild_is_not_ready_and_marks_index_untrusted() {
    // 5 批（1000/200），取消在第 3 批前生效 ⇒ 已写入 2 批 = 400 篇 < 1000
    let (guard, root) = build_vault(1000);
    let (db_guard, pool) = pool();
    let state = AppState::new();
    let seen = Mutex::new(Vec::<(usize, usize)>::new());
    let outcome = full_index_cancellable(
        &pool,
        &root,
        |done, total| seen.lock().expect("进度").push((done, total)),
        IndexMode::Force,
        || seen.lock().map(|v| v.len() >= 2).unwrap_or(false),
    )
    .expect("可取消的全量索引");
    assert!(outcome.cancelled, "取消必须被观测到");
    let total = seen.lock().expect("进度").first().map(|(_, t)| *t).unwrap();
    assert!(
        (outcome.indexed as usize) < total,
        "取消必须真的提前停下：indexed={} total={total}",
        outcome.indexed
    );

    let ending = settle_rebuild(&state, &pool, &outcome).expect("收尾判定");
    assert_eq!(ending, RebuildEnding::Cancelled);
    assert!(
        !state.index_ready(),
        "取消后**不得**宣称就绪（DEBT-45 的核心）"
    );
    let marker = pool
        .with_reader(|conn| read_meta(conn, META_REBUILD_IN_PROGRESS))
        .expect("读标记");
    assert!(
        marker.is_some(),
        "半索引必须留下 rebuild_in_progress 标记（ensure_index_db 据此重建）"
    );
    let _ = (&guard, &db_guard);
}

/// 成功 ⇒ 就绪、标记被清掉、收尾判定是 Completed。
#[test]
fn completed_rebuild_is_ready_and_clears_marker() {
    let (guard, root) = build_vault(20);
    let (db_guard, pool) = pool();
    let state = AppState::new();
    let outcome = full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false)
        .expect("全量索引");
    assert!(!outcome.cancelled);
    let ending = settle_rebuild(&state, &pool, &outcome).expect("收尾判定");
    assert_eq!(ending, RebuildEnding::Completed);
    assert!(state.index_ready(), "成功后必须宣称就绪");
    let marker = pool
        .with_reader(|conn| read_meta(conn, META_REBUILD_IN_PROGRESS))
        .expect("读标记");
    assert!(marker.is_none(), "成功后不得残留不可信标记");
    let _ = (&guard, &db_guard);
}

/// 事件语义：取消只发 `kp://index/cancelled`，**不得**发 `kp://index/completed`（命令用同一个函数分发）。
#[test]
fn cancelled_ending_emits_cancelled_event_not_completed() {
    assert_eq!(
        ending_event(RebuildEnding::Cancelled),
        "kp://index/cancelled"
    );
    assert_eq!(
        ending_event(RebuildEnding::Completed),
        "kp://index/completed"
    );
    assert_ne!(
        ending_event(RebuildEnding::Cancelled),
        "kp://index/completed",
        "取消不得复用 completed 事件（DEBT-45 的第二个缺陷）"
    );
}
