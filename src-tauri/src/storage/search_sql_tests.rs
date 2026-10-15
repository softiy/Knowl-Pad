//! storage::search_sql 的测试：**不需要数据库** —— 直接断言 SQL 片段与绑定参数。
//!
//! 判据对着需求：FR-SEARCH-20/21/22（三种过滤条件各落成什么谓词）、
//! FR-SEARCH-22 的层级前缀、FR-SEARCH-25 在纯过滤查询下的 NOT IN 改写，
//! 以及「空查询绝不退化成列出全库」。

use super::*;
use kp_domain::search::{parse_query, MatchMode};

fn spec(query: &str) -> QuerySpec {
    let parsed = parse_query(query, MatchMode::All).expect("解析");
    build_spec(&parsed, SortOrder::Relevance)
}

/// 反斜杠用码点构造：避免测试源码里堆转义符，读起来更清楚。
fn bs() -> char {
    char::from(92u8)
}

#[test]
fn escape_like_neutralizes_wildcards() {
    assert_eq!(escape_like("abc"), "abc", "普通字符原样保留");
    assert_eq!(escape_like("a_b"), format!("a{}_b", bs()), "下划线必须转义");
    assert_eq!(
        escape_like("100%"),
        format!("100{}%", bs()),
        "百分号必须转义"
    );
    assert_eq!(
        escape_like(&format!("a{}b", bs())),
        format!("a{0}{0}b", bs()),
        "反斜杠自身必须先转义（否则 ESCAPE 会吃掉它）"
    );
}

#[test]
fn path_and_file_become_bound_like_predicates() {
    let s = spec("path:work/ file:报告 x");
    assert!(
        s.from_where.contains("f.rel_path LIKE ?"),
        "{}",
        s.from_where
    );
    assert!(s.from_where.contains("f.name LIKE ?"), "{}", s.from_where);
    assert!(
        s.from_where.contains("ESCAPE"),
        "LIKE 必须带 ESCAPE：{}",
        s.from_where
    );
    assert!(
        s.params[0].contains('x'),
        "第一个绑定参数应是 MATCH 表达式：{:?}",
        s.params
    );
    assert!(s.params.contains(&"work/%".to_string()), "{:?}", s.params);
    assert!(s.params.contains(&"%报告%".to_string()), "{:?}", s.params);
}

#[test]
fn tag_filter_matches_the_tag_and_its_descendants() {
    let s = spec("tag:#工作/2026 内容");
    assert!(
        s.from_where.contains("EXISTS (SELECT 1 FROM file_tag"),
        "{}",
        s.from_where
    );
    assert!(
        s.params.contains(&"工作/2026".to_string()),
        "{:?}",
        s.params
    );
    assert!(
        s.params.contains(&"工作/2026/%".to_string()),
        "FR-SEARCH-22 的层级前缀匹配：{:?}",
        s.params
    );
}

#[test]
fn filter_only_query_lists_notes_without_match() {
    let s = spec("tag:#工作");
    assert!(
        !s.from_where.contains("note_fts MATCH"),
        "纯过滤不该有 MATCH：{}",
        s.from_where
    );
    assert!(s.from_where.contains("f.kind = 'note'"), "{}", s.from_where);
    assert!(s.select.contains("0.0 AS score"), "没有 bm25：{}", s.select);
    assert_eq!(
        s.order_by, "f.mtime_ms DESC, f.rel_path ASC",
        "相关度对纯过滤无意义"
    );
}

/// FR-SEARCH-25：纯过滤查询下 FTS5 的二元 NOT 用不了，必须改写成 NOT IN 子查询。
#[test]
fn filter_only_exclusions_use_not_in_subquery() {
    let s = spec("tag:#工作 -草稿");
    assert!(
        s.from_where
            .contains("f.id NOT IN (SELECT rowid FROM note_fts WHERE note_fts MATCH ?)"),
        "{}",
        s.from_where
    );
    assert!(
        s.params.iter().any(|p| p.contains("草稿")),
        "排除词必须作为绑定参数：{:?}",
        s.params
    );
}

/// **FR-SEARCH-25 + 2026-10-15 裁定**（PRD §5.3.4.3）：只有排除项时基集是**全部笔记**，
/// 语义 = 全集 − 含该词的笔记；**不得**退化成恒假条件（那会变成静默空结果）。
#[test]
fn exclude_only_uses_the_whole_note_set_as_base() {
    let s = spec("-草稿");
    assert!(
        !s.from_where.ends_with("WHERE 0"),
        "不得退化成恒假条件：{}",
        s.from_where
    );
    assert!(s.from_where.contains("f.kind = 'note'"), "{}", s.from_where);
    assert!(
        s.from_where
            .contains("f.id NOT IN (SELECT rowid FROM note_fts WHERE note_fts MATCH ?)"),
        "{}",
        s.from_where
    );
    assert!(
        s.params.iter().any(|p| p.contains("草稿")),
        "排除词必须作为绑定参数：{:?}",
        s.params
    );
}

/// 空查询（无正面词、无过滤条件、无排除项）**绝不能**退化成「列出全库」。
#[test]
fn empty_query_yields_a_false_condition() {
    let s = spec("   ");
    assert!(s.from_where.ends_with("WHERE 0"), "{}", s.from_where);
    assert!(s.params.is_empty(), "{:?}", s.params);
}

/// 排序仍由枚举决定（FR-SEARCH-06），过滤条件不会把 ORDER BY 换掉。
#[test]
fn sort_order_still_comes_from_the_enum() {
    let parsed = parse_query("tag:#工作 报告", MatchMode::All).expect("解析");
    assert_eq!(
        build_spec(&parsed, SortOrder::Path).order_by,
        "f.rel_path ASC"
    );
    assert!(build_spec(&parsed, SortOrder::Relevance)
        .order_by
        .starts_with("score ASC"));
}

/// 纯过滤查询**没有** bm25：相关度退化为修改时间倒序，但另外三种排序必须照常生效
/// （否则用户在纯 tag: 查询里选的「按路径」会被静默吃掉）。
#[test]
fn filter_only_query_keeps_the_requested_sort() {
    let parsed = parse_query("tag:#工作", MatchMode::All).expect("解析");
    assert_eq!(
        build_spec(&parsed, SortOrder::Path).order_by,
        "f.rel_path ASC"
    );
    assert!(build_spec(&parsed, SortOrder::Name)
        .order_by
        .starts_with("f.name COLLATE NOCASE"));
    assert_eq!(
        build_spec(&parsed, SortOrder::Mtime).order_by,
        "f.mtime_ms DESC, f.rel_path ASC"
    );
    assert_eq!(
        build_spec(&parsed, SortOrder::Relevance).order_by,
        "f.mtime_ms DESC, f.rel_path ASC",
        "相关度在没有 bm25 时退化为修改时间倒序"
    );
}
