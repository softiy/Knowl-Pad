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
use kp_domain::search::{parse_query, MatchMode, ParsedQuery};

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

/// 走**真实解析**（含高级语法），而不是手工拼表达式 —— 否则测试与用户输入会脱节。
fn parsed(query: &str) -> ParsedQuery {
    parse_query(query, MatchMode::All).expect("解析查询")
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
    let (hits, total) = fulltext_page(pool, &parsed(query), sort, limit, offset).expect("查询");
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
    let (hits, _) =
        fulltext_page(&pool, &parsed("图谱"), SortOrder::Relevance, 10, 0).expect("查询");
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

// ── 高级语法（FR-SEARCH-20~26）：命中集合必须对 ────────────────────────────

/// FR-SEARCH-20：path:folder/ 只保留该前缀下的笔记（同名文件前缀不算）。
#[test]
fn path_filter_limits_hits_to_the_prefix() {
    let (_d, pool) = vault(&[
        ("work/a.md", "季度 报告 内容\n"),
        ("work/2026/b.md", "季度 报告 内容\n"),
        ("other/c.md", "季度 报告 内容\n"),
        ("workbook.md", "季度 报告 内容\n"),
    ]);
    let (paths, total) = page(&pool, "path:work/ 季度", SortOrder::Path, 10, 0);
    assert_eq!(total, 2, "只有 work/ 下的两篇：{paths:?}");
    assert_eq!(
        paths,
        vec!["work/2026/b.md".to_string(), "work/a.md".to_string()]
    );
    assert!(
        !paths.contains(&"workbook.md".to_string()),
        "path:work/ 不该命中只是名字像的文件"
    );
}

/// FR-SEARCH-21：file:名称 按**文件名**匹配，正文里出现不算（不参与全文）。
#[test]
fn file_filter_matches_name_not_body() {
    let (_d, pool) = vault(&[
        ("季度报告.md", "正文里也有 季度 两个字\n"),
        ("随笔.md", "季度报告 只是正文里的一串字\n"),
    ]);
    let (paths, total) = page(&pool, "file:季度报告 季度", SortOrder::Path, 10, 0);
    assert_eq!(total, 1, "只有文件名匹配的那篇：{paths:?}");
    assert_eq!(paths, vec!["季度报告.md".to_string()]);
}

/// FR-SEARCH-22：tag:#工作 必须连 #工作/2026 的笔记一起命中（层级前缀匹配）。
#[test]
fn tag_filter_matches_descendant_tags() {
    let (_d, pool) = vault(&[
        ("a.md", "#工作/2026 内容\n"),
        ("b.md", "#工作 内容\n"),
        ("c.md", "#私事 内容\n"),
    ]);
    let (paths, total) = page(&pool, "tag:#工作 内容", SortOrder::Path, 10, 0);
    assert_eq!(total, 2, "祖先标签要连后代一起命中：{paths:?}");
    assert!(paths.contains(&"a.md".to_string()) && paths.contains(&"b.md".to_string()));
    let (deep, deep_total) = page(&pool, "tag:#工作/2026 内容", SortOrder::Path, 10, 0);
    assert_eq!(deep_total, 1, "指名到 2026 就只命中它自己：{deep:?}");
}

/// FR-SEARCH-26：标签 + 全文词 + 排除项混用时三个条件都生效。
#[test]
fn filters_mix_with_words_and_exclusions() {
    let (_d, pool) = vault(&[
        ("a.md", "#工作/2026 季度报告 正文\n"),
        ("b.md", "#工作/2026 季度报告 草稿\n"),
        ("c.md", "#私事 季度报告 正文\n"),
    ]);
    let (paths, total) = page(
        &pool,
        "tag:#工作/2026 季度报告 -草稿",
        SortOrder::Path,
        10,
        0,
    );
    assert_eq!(total, 1, "三个条件同时生效：{paths:?}");
    assert_eq!(paths, vec!["a.md".to_string()]);
}

/// 纯过滤查询（没有全文词）：按过滤条件列笔记，且不受「没有 MATCH」影响。
#[test]
fn filter_only_query_lists_notes_by_filter() {
    let (_d, pool) = vault(&[
        ("a.md", "#工作 甲\n"),
        ("b.md", "#工作 乙\n"),
        ("c.md", "#私事 丙\n"),
    ]);
    let (paths, total) = page(&pool, "tag:#工作", SortOrder::Path, 10, 0);
    assert_eq!(total, 2, "纯 tag: 查询应列出两篇：{paths:?}");
    assert_eq!(paths, vec!["a.md".to_string(), "b.md".to_string()]);
}

/// **FR-SEARCH-25 + 2026-10-15 裁定**（PRD §5.3.4.3）：`-词` 单独使用时
/// 结果集 = **全集 − 含该词的笔记**。断言的是**集合关系**，不是「不报错」。
#[test]
fn exclude_only_query_returns_the_complement() {
    let (_d, pool) = vault(&[
        (
            "a.md",
            "草稿 在这里
",
        ),
        (
            "b.md",
            "正常内容
",
        ),
        (
            "c.md",
            "另一篇正常内容
",
        ),
    ]);
    let (hits, hit_total) = page(&pool, "草稿", SortOrder::Path, 10, 0);
    assert_eq!(hit_total, 1, "含该词的恰好一篇：{hits:?}");
    let (complement, total) = page(&pool, "-草稿", SortOrder::Path, 10, 0);
    assert_eq!(total, 2, "补集应为两篇（3 − 1）：{complement:?}");
    assert_eq!(complement, vec!["b.md".to_string(), "c.md".to_string()]);
    assert!(
        !complement.iter().any(|p| hits.contains(p)),
        "补集与命中集不得相交：命中 {hits:?} / 补集 {complement:?}"
    );
}

/// FR-SEARCH-24 的回归：引号短语必须**只**命中连着写的那篇。
///
/// 这条用例同时锁住了 M3 期的一个真实缺陷：当时直接发 \"季度报告\"，
/// unicode61 把整串汉字当成一个 token → 实测 **0 命中**（2026-10-15 探针；
/// 改成 \"季度 报告\" 后 1 命中）。断言的是**命中集合**，不是「不报错」。
#[test]
fn quoted_phrase_requires_the_words_to_be_adjacent() {
    let (_d, pool) = vault(&[("a.md", "季度报告\n"), ("b.md", "季度和报告\n")]);
    let (paths, total) = page(&pool, "\"季度报告\"", SortOrder::Path, 10, 0);
    assert_eq!(total, 1, "短语只命中连着写的那篇：{paths:?}");
    assert_eq!(paths, vec!["a.md".to_string()]);
}
