//! 文件监听端到端测试（**AC-SEARCH-04**：外部改动后索引自动跟上）。

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::index_engine::full_index;
use crate::index_watch::{start_watcher_with, Change};
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

    let changes = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("应收到变更事件");
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
