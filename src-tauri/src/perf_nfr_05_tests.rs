//! **NFR-PERF-05 快速打开（后端部分）的三档实测**（2026-10-28）。
//!
//! 口径：PRD §6.1.4 + 勘误 D-30 —— 快速打开 = 后端 search_quick_open（Rust Instant）+ 前端首屏渲染
//! （performance.mark），判定取两段合计。本用例测**后端段**（含索引检索与路径匹配，不含 IPC/渲染）；
//! 前端段在 jsdom 下无意义，如实登记为真机项。
//!
//! 三档预算（PRD NFR-PERF-05 原文）：< 30ms / < 50ms / < 100ms（标准 / 大 / 超大）。
//! 每档 30 次取中位数；打印一行供采样读取。
//! 规模：标准档 3000 篇在 CI 也跑；20000 / 100000 标 #[ignore]（与 d25 大数据档同一惯例）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::commands::search::QuickOpenArgs;
use crate::commands::search_index::search_quick_open_impl;
use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::state::AppState;
use crate::storage::index::open;

const ITERATIONS: usize = 30;

fn write_note(root: &Path, i: usize) {
    let rel = format!("d{}/note-{i}.md", i % 20);
    let path = root.join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("建目录");
    }
    std::fs::write(&path, format!("# 笔记 {i}\n\n第 {i} 篇，快速打开夹具。\n")).expect("写笔记");
}

fn vault(notes: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..notes {
        write_note(&root, i);
    }
    (dir, root)
}

fn state_for(root: &Path) -> (tempfile::TempDir, AppState) {
    let db_dir = tempfile::tempdir().expect("索引库临时目录");
    let pool = Arc::new(open(&db_dir.path().join("index.db")).expect("打开索引库"));
    full_index_cancellable(&pool, root, |_, _| {}, IndexMode::Force, || false).expect("全量索引");
    let state = AppState::new();
    state.set_root(Some(root.to_path_buf()));
    state.set_index_db(Some(pool));
    (db_dir, state)
}

fn median_ms(state: &AppState, query: &str) -> u128 {
    let args = || QuickOpenArgs {
        query: query.to_string(),
        limit: Some(20),
    };
    // 热身：排除一次性初始化（与 d25_nfr_perf_04_search 同口径）
    let _ = search_quick_open_impl(state, args()).expect("快速打开热身");
    let mut samples: Vec<u128> = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        let items = search_quick_open_impl(state, args()).expect("快速打开");
        samples.push(started.elapsed().as_micros());
        assert!(!items.is_empty(), "查询 note 在夹具上必须命中");
    }
    samples.sort_unstable();
    samples[ITERATIONS / 2] / 1000
}

fn run_tier(tier: usize, budget_ms: u128) {
    let (guard, root) = vault(tier);
    let (db_guard, state) = state_for(&root);
    let ms = median_ms(&state, "note");
    let verdict = if ms < budget_ms { "PASS" } else { "FAIL" };
    println!(
        "D25 NFR-PERF-05-quick-open tier={tier} ms={ms} budget_ms={budget_ms} verdict={verdict} notes={tier} iters={ITERATIONS} scope=backend path=command query=note"
    );
    let _ = (&guard, &db_guard);
}

/// 标准档（3000 篇 / 30ms）：CI 也跑。
#[test]
fn d25_nfr_perf_05_quick_open_3000() {
    run_tier(3000, 30);
}

/// 大档（20000 篇 / 50ms）：--ignored 显式执行。
#[test]
#[ignore = "20000 篇要真写盘与真索引；显式 --ignored 执行（见 perf/d25-budgets.json）"]
fn d25_nfr_perf_05_quick_open_20000() {
    run_tier(20000, 50);
}

/// 超大档（100000 篇 / 100ms）：--ignored 显式执行。
#[test]
#[ignore = "100000 篇要真写盘与真索引；显式 --ignored 执行（见 perf/d25-budgets.json）"]
fn d25_nfr_perf_05_quick_open_100000() {
    run_tier(100000, 100);
}
