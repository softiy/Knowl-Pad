//! kp_domain::search 的测试（单独成文件：与 error_tests / find_in_file_tests 同一约定，
//! 也让 search.rs 留在 CODE-11 的 200 SLOC 预警线以内）。

use super::*;

fn expr(query: &str, mode: MatchMode) -> String {
    build_match_expr(query, mode).expect("正常查询不该报错")
}

#[test]
fn escaping_wraps_in_quotes_and_doubles_inner() {
    assert_eq!(escape_fts("a\"b"), "\"a\"\"b\"");
    assert_eq!(escape_fts("a:b*c"), "\"a:b*c\"");
}

/// FR-SEARCH-24：短语整体作为 FTS5 **短语查询**发出。
///
/// 注意这里的空格：不能直接发 \"季度报告\"（unicode61 会把它当成一个 token，实测 0 命中），
/// 必须先按 jieba 切开再连接 —— 详见 phrase_expr 的注释。
#[test]
fn phrase_is_kept_whole_and_sent_as_a_phrase_query() {
    assert_eq!(expr("\"季度报告\"", MatchMode::All), "\"季度 报告\"");
    assert_eq!(
        expr("\"力导向布局\"", MatchMode::All),
        format!("\"{}\"", tokenize::for_query("力导向布局").join(" ")),
        "短语 = 分词结果按原顺序拼成的短语查询"
    );
}

#[test]
fn all_mode_uses_and_any_mode_uses_or() {
    let a = expr("markdown test", MatchMode::All);
    assert!(a.contains(" AND "), "默认 AND：{a}");
    let o = expr("markdown test", MatchMode::Any);
    assert!(o.contains(" OR "), "可选 OR：{o}");
}

#[test]
fn exclusions_use_chained_not() {
    let e = expr("报告 -草稿 -废弃", MatchMode::All);
    assert!(e.contains("NOT"), "应有排除：{e}");
    assert_eq!(e.matches("NOT").count(), 2, "多个排除词要链式 NOT：{e}");
    assert!(e.contains("草稿") && e.contains("废弃"));
}

#[test]
fn empty_query_yields_empty_expr() {
    assert_eq!(expr("   ", MatchMode::All), "");
    assert!(parse_query("   ", MatchMode::All)
        .expect("空查询")
        .is_empty_query());
}

#[test]
fn special_chars_do_not_break_syntax() {
    // 用户输入含 FTS5 元字符时不得产生语法错误（由 escape 保证）
    let e = expr("a*b (c) ^d", MatchMode::All);
    assert!(e.starts_with('"') || e.is_empty(), "表达式：{e}");
    assert!(!e.contains("(c)"), "括号必须被引号包裹：{e}");
}

/// **FR-SEARCH-25 + 2026-10-15 项目所有者裁定**（口径已落 PRD §5.3.4.3）：
/// `-词` 单独使用时语义是**排除** = 全部笔记 − 含该词的笔记。
///
/// 这条用例**替换**了 M3 期的 `only_excludes_still_yields_a_usable_expr`：那时实现把
/// 「只有排除项」退化成「按包含处理」（`-草稿` 返回**含**草稿的笔记），与 FR-SEARCH-25
/// 的原文「`-词` 排除含该词的结果」正好相反，本刀按裁定改正并在此写明依据。
#[test]
fn only_excludes_is_a_pure_exclusion_query() {
    let parsed = parse_query("-草稿", MatchMode::All).expect("解析");
    assert!(parsed.positive_expr.is_empty(), "没有正面词");
    assert_eq!(parsed.excludes, vec!["草稿".to_string()], "排除词已分词");
    assert!(
        parsed.match_expr.is_empty(),
        "没有正面词就没有 MATCH（FTS5 的 NOT 缺左侧操作数）：{}",
        parsed.match_expr
    );
    assert!(
        !parsed.is_empty_query(),
        "它不是空查询：语义是「全部笔记 − 含草稿的笔记」，必须真的去列笔记"
    );
    assert!(parsed.highlight.is_empty(), "排除词不入高亮");
    // 有正面词时仍是链式 NOT（未受影响）
    let mixed = parse_query("报告 -草稿", MatchMode::All).expect("解析");
    assert!(mixed.match_expr.contains("NOT"), "{}", mixed.match_expr);
}

#[test]
fn match_mode_parses_wire_values() {
    assert_eq!(MatchMode::parse("all"), Some(MatchMode::All));
    assert_eq!(MatchMode::parse("any"), Some(MatchMode::Any));
    assert_eq!(
        MatchMode::parse("ALL"),
        None,
        "线上取值区分大小写，未知由调用方取默认"
    );
    assert_eq!(MatchMode::parse(""), None);
}

#[test]
fn highlight_terms_keep_phrases_and_positive_words_only() {
    let terms = highlight_terms("\"季度报告\" 工作 -草稿").expect("解析");
    assert!(
        terms.contains(&"季度报告".to_string()),
        "短语整体保留：{terms:?}"
    );
    assert!(terms.contains(&"工作".to_string()));
    assert!(
        !terms.iter().any(|t| t == "草稿"),
        "排除词不入高亮：{terms:?}"
    );
}

#[test]
fn highlight_terms_dedupe_repeats() {
    assert_eq!(
        highlight_terms("图谱 图谱 图谱").expect("解析"),
        vec!["图谱".to_string()],
        "重复词只留一份"
    );
}

#[test]
fn highlight_terms_for_empty_query_is_empty() {
    assert!(highlight_terms("   ").expect("解析").is_empty());
    assert!(
        highlight_terms("-草稿").expect("解析").is_empty(),
        "只有排除词时没有可高亮的正面词"
    );
}

/// FR-SEARCH-20~23：过滤条件**不得**混进 MATCH（否则 path:work 会变成搜 path 与 work 两个词）。
#[test]
fn filters_are_not_part_of_the_match_expression() {
    let parsed = parse_query("tag:#工作 报告", MatchMode::All).expect("解析");
    assert_eq!(parsed.filters.tags, vec!["工作".to_string()]);
    assert_eq!(parsed.match_expr, "\"报告\"", "标签不得进 MATCH");
    assert_eq!(parsed.positive_expr, "\"报告\"");
}

/// 纯过滤查询：没有全文正面词，positive_expr 必须为空，storage 才能走过滤列表路径。
#[test]
fn filter_only_query_has_no_positive_expr() {
    let parsed = parse_query("tag:#工作", MatchMode::All).expect("解析");
    assert!(parsed.positive_expr.is_empty(), "{parsed:?}");
    assert!(parsed.match_expr.is_empty(), "没有排除项时完整表达式为空");
    assert!(
        !parsed.is_empty_query(),
        "纯过滤查询不是空查询：必须真的去列笔记"
    );
}

/// FR-SEARCH-27 / AC-SEARCH-05：未闭合引号必须是语法错误，而不是被静默当普通词。
#[test]
fn unclosed_quote_is_a_syntax_error() {
    let err = build_match_expr("\"未闭合的短语", MatchMode::All).expect_err("必须报语法错误");
    assert_eq!(err.code(), "E_SEARCH_SYNTAX");
    assert!(err.to_string().contains("双引号未闭合"), "{err}");
    assert!(
        !err.to_string().contains("未闭合的短语"),
        "错误文案不得回显用户查询串（SEC-09 的同一纪律）：{err}"
    );
}

/// FR-SEARCH-26：过滤条件、精确短语、排除词三者混用时各归各位。
#[test]
fn filters_mix_with_phrase_and_exclusion() {
    let parsed = parse_query("tag:#工作/2026 \"季度报告\" -草稿", MatchMode::All).expect("解析");
    assert_eq!(parsed.filters.tags, vec!["工作/2026".to_string()]);
    assert!(
        parsed.match_expr.contains(&phrase_expr("季度报告")),
        "短语按分词后的短语查询发出：{}",
        parsed.match_expr
    );
    assert!(parsed.match_expr.contains("NOT \"草稿\""));
    assert_eq!(parsed.highlight, vec!["季度报告".to_string()]);
}
