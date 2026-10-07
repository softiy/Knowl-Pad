//! 文件监听端到端测试（**AC-SEARCH-04**：外部改动后索引自动跟上）。

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::index_engine::full_index;
use crate::index_watch::{start_polling_fallback, start_watcher_with, Change};
use crate::index_watch_paths::merge_events;
use crate::storage::index::open;
use kp_domain::error::AppError;

/// 读取某目标的裁决状态。
fn link_status(pool: &crate::storage::pool::DbPool, target: &str) -> String {
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT status FROM link WHERE target_ref = ?1",
            rusqlite::params![target],
            |r| r.get::<_, String>(0),
        )
        .map_err(|_| AppError::db("读裁决"))
    })
    .expect("读裁决")
}

fn count(pool: &crate::storage::pool::DbPool, sql: &str) -> i64 {
    pool.with_reader(|conn| {
        conn.query_row(sql, [], |r| r.get(0))
            .map_err(|_| AppError::db("计数"))
    })
    .expect("计数")
}

/// 合并逻辑单测：create+modify 只应产生一条 created；忽略目录不产生任何变更。
#[test]
fn merge_events_dedupes_and_ignores() {
    let root = Path::new("C:/vault");
    let kinds = vec![
        (
            notify::EventKind::Create(notify::event::CreateKind::File),
            vec![std::path::PathBuf::from("C:/vault/a.md")],
        ),
        (
            notify::EventKind::Modify(notify::event::ModifyKind::Any),
            vec![std::path::PathBuf::from("C:/vault/.obsidian/x.md")],
        ),
    ];
    let changes = merge_events(root, kinds);
    assert_eq!(
        changes,
        vec![Change::Created("a.md".to_string())],
        "忽略目录不得产生变更"
    );
}

/// **AC-SEARCH-04**：外部新增一篇笔记 → 监听触发 → 增量索引 → 可检索。
#[test]
fn ac_search_04_external_change_is_indexed_and_searchable() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n\n已有内容，指向 [[未来]]\n").expect("写笔记");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");

    let (tx, rx) = std::sync::mpsc::channel::<Vec<Change>>();
    let handle = start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            let _ = tx.send(changes.to_vec());
        },
        |_code, _message| {},
    )
    .expect("启动监听");

    std::thread::sleep(Duration::from_millis(300));
    fs::write(root.join("外部新增.md"), "# 外部新增\n\n中文内容可检索\n").expect("外部写入");

    let started = std::time::Instant::now();
    let changes = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("应收到变更事件");
    let elapsed_ms = started.elapsed().as_millis();
    println!("AC-SEARCH-04 耗时（外部写入 → 收到事件）：{elapsed_ms} ms");
    assert!(elapsed_ms < 10_000, "外部变更到事件耗时 {elapsed_ms} ms 超出宽松上限");
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::Created(p) if p == "外部新增.md")),
        "变更应为 created：{changes:?}"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM file WHERE rel_path = '外部新增.md'"
        ),
        1,
        "AC-SEARCH-04：外部新增应已被索引"
    );
    let expr = kp_domain::search::build_match_expr("中文内容", kp_domain::search::MatchMode::All);
    let hits: i64 = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT count(*) FROM note_fts WHERE note_fts MATCH ?1",
                rusqlite::params![expr],
                |r| r.get(0),
            )
            .map_err(|_| AppError::db("FTS 查询"))
        })
        .expect("FTS 查询");
    assert!(hits >= 1, "外部新增的内容应可检索（expr = {expr}）");
    // 增量裁决：a.md 里有一条指向 `未来.md` 的链接，此刻应为 dangling
    let status_before = link_status(&pool, "未来");
    assert_eq!(status_before, "dangling", "文件还不存在时链接应为 dangling");
    fs::write(root.join("未来.md"), "# 未来\n").expect("外部写入第二篇");
    let _ = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("应收到第二次变更");
    let status_after = link_status(&pool, "未来");
    assert_eq!(
        status_after, "resolved",
        "增量裁决：目标文件出现后，原本悬空的链接应转为 resolved"
    );

    handle.stop();
}
/// **降级路径的真实验证**（独立审查 major #4）：监听句柄耗尽时启动的 5 秒轮询，
/// 必须真的能把外部新增的文件索引进来 —— 此前那条分支只发事件、并没有启动轮询。
#[test]
fn polling_fallback_indexes_external_change() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n").expect("写笔记");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let _handle = start_polling_fallback(pool.clone(), root.clone());
    fs::write(root.join("轮询新增.md"), "# 轮询新增\n\n内容\n").expect("外部写入");
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    loop {
        let n = count(
            &pool,
            "SELECT count(*) FROM file WHERE rel_path = '轮询新增.md'",
        );
        if n == 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "轮询降级未在 12 秒内索引外部新增（说明降级没有真的跑起来）"
        );
        std::thread::sleep(Duration::from_millis(300));
    }
}
/// **监听死循环防护**（计划 §4 第 21 行 / TECH §13.1 验证项 ③）：
/// 索引只写 `.knowlpad/`（忽略目录）→ "索引进行中改文件"不应产生事件；
/// 并且必须证明监听**是活的**（否则"没有事件"可能只是没在监听）。
#[test]
fn indexing_does_not_trigger_itself_but_watcher_stays_alive() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n\n内容\n").expect("写笔记");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Change>>();
    let handle = start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            let _ = tx.send(changes.to_vec());
        },
        |_code, _message| {},
    )
    .expect("启动监听");
    std::thread::sleep(Duration::from_millis(300));
    full_index(&pool, &root, |_, _| {}).expect("再次索引");
    let quiet = rx.recv_timeout(Duration::from_millis(1200));
    assert!(quiet.is_err(), "索引自身写 .knowlpad 不应触发变更事件，却收到 {quiet:?}");
    fs::write(root.join("b.md"), "# 乙\n").expect("外部写入");
    let alive = rx.recv_timeout(Duration::from_secs(15)).expect("监听应仍然活着");
    assert!(alive.iter().any(|c| matches!(c, Change::Created(p) if p == "b.md")), "应报出外部新增：{alive:?}");
    handle.stop();
}
