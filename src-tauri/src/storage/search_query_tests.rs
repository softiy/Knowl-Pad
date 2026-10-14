//! 搜索域索引侧查询的测试：**真实临时索引库 + 真实夹具笔记**（走 full_index，不灌桩数据）。
//!
//! 判据对着需求：AC-SEARCH-01（中文短语命中）、FR-SEARCH-06（排序）、
//! FR-SEARCH-40（只搜文件名）、IPC-05 的分页与总数一致性。

use rusqlite::params;

use crate::index_engine::full_index;
use crate::storage::index::open;
use crate::storage::pool::DbPool;
use crate::storage::search_query::{fulltext_page, quick_open_candidates, SortOrder};
use kp_domain::error::AppError;
use kp_domain::search::{build_match_expr, MatchMode};

/// 建临时 Vault → 建索引库 → 全量索引。
fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).expect("建目录");
        }
        std::fs::write(&path, content).expect("写文件");
    }
    let pool = open(&root).expect("建索引库");
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    (dir, pool)
}

fn expr(query: &str) -> String {
    build_match_expr(query, MatchMode::All)
}

fn set_mtime(pool: &DbPool, rel: &str, mtime: i64) {
    let rel = rel.to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "UPDATE file SET mtime_ms = ?1 WHERE rel_path = ?2",
            params![mtime, rel],
        )
        .map(|_| ())
        .map_err(|e| AppError::db_with("改 mtime", &e))
    })
    .expect("改 mtime");
}

fn page(
    pool: &DbPool,
    query: &str,
    sort: SortOrder,
    limit: i64,
    offset: i64,
) -> (Vec<String>, i64) {
    let (hits, total) = fulltext_page(pool, &expr(query), sort, limit, offset).expect("查询");
    (hits.into_iter().map(|h| h.rel_path).collect(), total)
}

/// AC-SEARCH-01 的索引侧形状：中文短语经 jieba 预分词后必须能被查询侧命中的同一批词命中。
#[test]
fn chinese_phrase_hits_only_the_notes_that_contain_it() {
    let (_d, pool) = vault(&[
        ("a.md", "知识图谱的力导向布局很关键\n"),
        ("sub/b.md", "另一篇也讲知识图谱的力导向布局\n"),
        ("c.md", "只提到力导向布局\n"),
        ("d.md", "毫不相关的内容\n"),
    ]);
    let (paths, total) = page(&pool, "力导向布局", SortOrder::Relevance, 10, 0);
    assert_eq!(total, 3, "三篇命中");
    assert_eq!(paths.len(), 3, "total 与本页条数一致");
    for want in ["a.md", "sub/b.md", "c.md"] {
        assert!(
            paths.contains(&want.to_string()),
            "{want} 应命中：{paths:?}"
        );
    }
    assert!(!paths.contains(&"d.md".to_string()), "无关笔记不得命中");
}

/// 多词 AND（FTS-02 默认）：只命中同时含两个词的笔记。
#[test]
fn multi_word_query_uses_and_semantics() {
    let (_d, pool) = vault(&[
        ("both.md", "图谱 与 布局 都在这里\n"),
        ("only-one.md", "这里只有 图谱\n"),
    ]);
    let (paths, total) = page(&pool, "图谱 布局", SortOrder::Relevance, 10, 0);
    assert_eq!(total, 1, "AND 语义");
    assert_eq!(paths, vec!["both.md".to_string()]);
}

/// FR-SEARCH-06：相关度排序按 bm25 升序（bm25 返回负值，越小越相关）。
#[test]
fn relevance_sort_is_ascending_by_bm25() {
    let (_d, pool) = vault(&[("many.md", "图谱 图谱 图谱 图谱\n"), ("one.md", "图谱\n")]);
    let (hits, _) = fulltext_page(&pool, &expr("图谱"), SortOrder::Relevance, 10, 0).expect("查询");
    assert!(
        hits.windows(2).all(|w| w[0].score <= w[1].score),
        "bm25 必须升序：{:?}",
        hits.iter().map(|h| h.score).collect::<Vec<_>>()
    );
}

/// FR-SEARCH-06：四种排序给出**可区分**的顺序（同分决胜键是路径，保证分页稳定）。
#[test]
fn sort_orders_are_observable_and_mtime_wins_when_set() {
    let (_d, pool) = vault(&[
        ("zzz.md", "图谱 内容\n"),
        ("aaa.md", "图谱 内容\n"),
        ("mmm.md", "图谱 内容\n"),
    ]);
    set_mtime(&pool, "zzz.md", 300);
    set_mtime(&pool, "aaa.md", 100);
    set_mtime(&pool, "mmm.md", 200);

    let (by_path, _) = page(&pool, "图谱", SortOrder::Path, 10, 0);
    assert_eq!(by_path, vec!["aaa.md", "mmm.md", "zzz.md"], "路径升序");
    let (by_name, _) = page(&pool, "图谱", SortOrder::Name, 10, 0);
    assert_eq!(by_name, vec!["aaa.md", "mmm.md", "zzz.md"], "文件名升序");
    let (by_mtime, _) = page(&pool, "图谱", SortOrder::Mtime, 10, 0);
    assert_eq!(
        by_mtime,
        vec!["zzz.md", "mmm.md", "aaa.md"],
        "修改时间倒序（新的在前）"
    );
}

/// 分页：total 是总数而不是本页条数；逐页取完必须**不重不漏**。
#[test]
fn pagination_slices_without_gaps_or_duplicates() {
    let (_d, pool) = vault(&[
        ("p1.md", "图谱\n"),
        ("p2.md", "图谱\n"),
        ("p3.md", "图谱\n"),
        ("p4.md", "图谱\n"),
        ("p5.md", "图谱\n"),
    ]);
    let (first, total) = page(&pool, "图谱", SortOrder::Path, 2, 0);
    assert_eq!(total, 5, "total 为命中总数");
    assert_eq!(first, vec!["p1.md", "p2.md"]);
    let (second, _) = page(&pool, "图谱", SortOrder::Path, 2, 2);
    assert_eq!(second, vec!["p3.md", "p4.md"]);
    let (third, _) = page(&pool, "图谱", SortOrder::Path, 2, 4);
    assert_eq!(third, vec!["p5.md"]);
    let (empty, _) = page(&pool, "图谱", SortOrder::Path, 2, 6);
    assert!(empty.is_empty(), "越界页应为空而非报错");
}

/// 软删除的笔记不得出现在搜索结果里（file.deleted = 1）。
#[test]
fn deleted_notes_are_excluded_from_results() {
    let (_d, pool) = vault(&[("keep.md", "图谱\n"), ("gone.md", "图谱\n")]);
    let (_before, total_before) = page(&pool, "图谱", SortOrder::Path, 10, 0);
    assert_eq!(total_before, 2);
    pool.with_writer(|conn| {
        conn.execute("UPDATE file SET deleted = 1 WHERE rel_path = 'gone.md'", [])
            .map(|_| ())
            .map_err(|e| AppError::db_with("标记软删除", &e))
    })
    .expect("标记软删除");
    let (after, total_after) = page(&pool, "图谱", SortOrder::Path, 10, 0);
    assert_eq!(total_after, 1, "软删除后总数应减少");
    assert_eq!(after, vec!["keep.md".to_string()]);
}

/// 没有任何命中时 total = 0 且 items 为空（不是报错）。
#[test]
fn no_match_yields_empty_page() {
    let (_d, pool) = vault(&[("a.md", "图谱\n")]);
    let (paths, total) = page(&pool, "完全不存在的词", SortOrder::Relevance, 10, 0);
    assert!(paths.is_empty());
    assert_eq!(total, 0);
}

/// FR-SEARCH-40：快速打开只列**笔记**，且按路径升序（供打分器稳定排序）。
#[test]
fn quick_open_candidates_are_notes_only_in_path_order() {
    let (_d, pool) = vault(&[
        ("note-b.md", "乙\n"),
        ("note-a.md", "甲\n"),
        ("sub/note-c.md", "丙\n"),
        ("图片.png", "不是笔记\n"),
    ]);
    let rows = quick_open_candidates(&pool).expect("候选");
    let paths: Vec<&str> = rows.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec!["note-a.md", "note-b.md", "sub/note-c.md"],
        "只列笔记、按路径升序"
    );
    assert_eq!(rows[0].1, "note-a.md", "name 与 rel_path 同时返回");
    assert!(
        !paths.iter().any(|p| p.ends_with(".png")),
        "附件不进快速打开"
    );
}
