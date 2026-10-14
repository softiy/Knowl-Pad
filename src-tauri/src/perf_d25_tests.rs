//! D-25 性能设施的**度量用例**（M5 批次「性能设施」，2026-10-13）。
//!
//! 约定：
//!   - 只**度量与打印**，不做严格阈值断言（严格判据由 `scripts/perf-d25.mjs` 按 PRD §6.1.2 的预算比对）；
//!   - 打印格式必须是 `D25 <判据 ID> key=value ...`（采样器按此解析）；
//!   - 夹具来源：环境变量 `KP_D25_VAULT` 指向 `scripts/gen-fixture-vault.mjs` 生成的库时直接使用，
//!     否则在临时目录里造一个小规模库 —— 这样 CI 跑得起，真机验收又能用真夹具。
//!
//! 覆盖现状见 `perf/d25-budgets.json` 的 `instrumented` 字段（未接入的判据由采样器显式打印为"待接入"）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::storage::index::open;
use crate::storage::pool::DbPool;

/// 造一篇笔记（含 frontmatter / 标题 / 链接 / 标签，贴近真实内容形状）。
fn write_note(root: &Path, rel: &str, i: usize) {
    let p = root.join(rel);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).expect("建目录");
    }
    let mut body = String::from("---\ntags: [a, b/c]\n---\n\n# 笔记 ");
    body.push_str(&i.to_string());
    body.push_str("\n\n");
    for k in 0..40 {
        body.push_str("内容 ");
        body.push_str(&(i * 131 + k).to_string());
        body.push_str(" 与 [[笔记 ");
        body.push_str(&((i + k) % 200).to_string());
        body.push_str("]] 以及 #标签/");
        body.push_str(&(i % 20).to_string());
        body.push('\n');
    }
    fs::write(p, body).expect("写笔记");
}

/// 夹具：优先用 `KP_D25_VAULT`（gen-fixture-vault.mjs 的产物），否则造一个临时小库。
fn d25_vault(notes: usize) -> (Option<tempfile::TempDir>, PathBuf) {
    if let Ok(p) = std::env::var("KP_D25_VAULT") {
        let path = PathBuf::from(p);
        assert!(path.is_dir(), "KP_D25_VAULT 不是目录：{path:?}");
        return (None, path);
    }
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..notes {
        write_note(&root, &format!("d{}/note-{i}.md", i % 20), i);
    }
    (Some(dir), root)
}

fn pool_for(root: &Path) -> DbPool {
    let db = root.join(".knowlpad").join("index.db");
    fs::create_dir_all(db.parent().expect("父目录")).expect("建 .knowlpad");
    open(&db).expect("打开索引库")
}

/// **NFR-PERF-03**：首次全量索引（含分词与 FTS 构建）。
#[test]
fn d25_nfr_perf_03_first_index() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    let started = Instant::now();
    let outcome =
        full_index_cancellable(&pool, &root, |_done, _total| {}, IndexMode::Force, || false)
            .expect("全量索引");
    let ms = started.elapsed().as_millis();
    let _ = &guard;
    println!(
        "D25 NFR-PERF-03-first-index ms={ms} notes={} indexed={} warnings={} engine_ms={}",
        notes,
        outcome.indexed,
        outcome.warnings.len(),
        outcome.duration_ms
    );
    // 宽松上限：只拦"数量级退化"（严格判据由采样器按 PRD 预算判定）。
    assert!(ms < 600_000, "首次全量索引 {ms} ms，已属数量级退化");
}

/// **AC-VAULT-05**：大 Vault 的进度事件与取消（取消必须被观测到，且发生在**批间**）。
#[test]
fn d25_ac_vault_05_progress_and_cancel() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    let events = AtomicUsize::new(0);
    let total_seen = AtomicUsize::new(0);
    let outcome = full_index_cancellable(
        &pool,
        &root,
        |_done, total| {
            events.fetch_add(1, Ordering::Relaxed);
            total_seen.store(total, Ordering::Relaxed);
        },
        IndexMode::Force,
        || events.load(Ordering::Relaxed) >= 1, // 第一批之后立刻要求取消
    )
    .expect("可取消的全量索引");
    let _ = &guard;
    println!(
        "D25 AC-VAULT-05-progress events={} total={} cancel-observed={} indexed={}",
        events.load(Ordering::Relaxed),
        total_seen.load(Ordering::Relaxed),
        outcome.cancelled,
        outcome.indexed
    );
    assert!(events.load(Ordering::Relaxed) >= 1, "至少要有一个进度事件");
    assert!(outcome.cancelled, "取消请求必须被观测到（AC-VAULT-05）");
}
