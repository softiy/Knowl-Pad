//! **NFR-PERF-11（端到端，后端 + 真实文件监听）**：外部写入完成 → 监听触发（200ms 防抖）→ 增量索引提交 →
//! `kp://fs/*` 事件 emit。PRD §6.1.4 把自动判据的**止点**定在「索引已可见且变更事件已 emit」
//! （§6.1.2 的「到 UI 反映」那一跳由端到端/手工验收覆盖）—— 本用例正是这个止点，**不假装到 UI**。
//!
//! 与 proxy（`perf_d25_tests::d25_nfr_perf_11_external_change_proxy`）的区别：proxy 是"手工调用一次增量扫描"，
//! 不含监听触发；这条从**真实 fs 写入**开始计时，经 `start_watcher_with` 的防抖与增量写库，到事件回调为止。
//!
//! 就绪同步（台账 T-22 的替代）：先写一个哨兵文件并等它的事件，确保监听已注册；否则注册延迟会被算进指标。
//! **T-22（监听就绪的显式信号）仍无独立 API**，本用例用哨兵同步并登记为边界。

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::storage::index::open;
use crate::storage::pool::DbPool;

/** 小夹具：`n` 篇笔记，分散在 d0..d9（一次外部变更只索引一个文件，指标与全库规模弱相关）。 */
fn small_vault(n: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..n {
        let rel = format!("d{}/note-{i}.md", i % 10);
        let path = root.join(&rel);
        fs::create_dir_all(path.parent().expect("父目录")).expect("建目录");
        fs::write(&path, format!("# 笔记 {i}\n\n内容 [[d0/note-0]]\n")).expect("写笔记");
    }
    (dir, root)
}

fn fresh_pool() -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("索引库临时目录");
    let pool = open(&dir.path().join("index.db")).expect("打开索引库");
    (dir, pool)
}

#[test]
fn d25_nfr_perf_11_external_change_end_to_end() {
    let (guard, root) = small_vault(200);
    let (db_guard, pool) = fresh_pool();
    full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false).expect("首轮索引");
    let pool = Arc::new(pool);

    let (tx, rx) = mpsc::channel::<crate::index_watch::Change>();
    let handle = crate::index_watch::start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            for change in changes {
                let _ = tx.send(change.clone());
            }
        },
        |_code, _message| {},
    )
    .expect("启动监听");

    // ① 就绪：哨兵文件的事件必须先到（否则后面的计时会把"监听刚注册"算进去）
    let sentinel = root.join("d0/sentinel-ready.md");
    fs::write(&sentinel, "# 就绪哨兵\n").expect("写哨兵");
    assert!(
        rx.recv_timeout(Duration::from_secs(10)).is_ok(),
        "监听必须在 10s 内就绪（哨兵事件）"
    );

    // ② 计时：外部写入（不经过本进程的写命令）→ 事件到达（`apply_changes` 先 commit 再 on_event）
    let target = root.join("d0/note-external.md");
    let started = Instant::now();
    fs::write(&target, "# 外部新增\n\n[[d1/note-1]] #外部/变更\n").expect("外部写入");
    // 等到**指向该文件**的事件（Windows 上目录自身的 Modified 也会来，不能拿目录事件充数）
    let target_rel = "d0/note-external.md";
    let deadline = Duration::from_secs(10);
    let mut change = None;
    while started.elapsed() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(candidate) => {
                let text = format!("{candidate:?}");
                if text.contains(target_rel) {
                    change = Some(candidate);
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let change = change.expect("外部变更必须在 10s 内到达（且事件指向该文件）");
    let ms = started.elapsed().as_millis();

    // ③ 止点断言：索引已可见（读得到该笔记的出链）+ 事件已 emit
    let rows = crate::storage::link_query::outgoing_for(&pool, "d0/note-external.md")
        .expect("读外部新增笔记的出链");
    assert!(!rows.is_empty(), "外部变更必须已进索引（事件到达时已提交）");
    println!(
        "D25 NFR-PERF-11-external-change ms={ms} visible=true budget_ms=1000 scope=watcher+index(no-ui) event={change:?}"
    );
    assert!(
        ms < 1000,
        "标准档预算 1000 ms（PRD §6.1.2 NFR-PERF-11），实测 {ms} ms"
    );

    handle.stop();
    let _ = (&guard, &db_guard);
}
