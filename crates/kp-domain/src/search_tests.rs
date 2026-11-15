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

/// **M5 冒烟 A7**：`词1 OR 词2`（匹配模式保持默认 AND）与「同一串词 + 模式 any」
/// 必须是**同一个 MATCH 表达式** —— 验收原文要求两种入口结果一致。
///
/// 一致不是巧合：行内 OR 只做一件事 —— 把 effective mode 切成 Any，MATCH 构造仍只有一条路径。
#[test]
fn inline_or_keyword_equals_any_mode() {
    let inline = expr("词1 OR 词2", MatchMode::All);
    let any = expr("词1 词2", MatchMode::Any);
    assert_eq!(inline, any, "行内 OR 与 any 模式必须是同一个表达式");
    assert!(inline.contains(" OR "), "表达式：{inline}");
    assert!(
        !inline.contains(" AND "),
        "整条查询切到 OR 后不该再有 AND：{inline}"
    );
    assert!(
        !inline.to_lowercase().contains("\"or\""),
        "OR 是运算符、不是检索词，不得进 MATCH：{inline}"
    );
}

/// 行内 OR 只切「正面词」的模式，**过滤条件与排除项**照旧：
/// 短语整段保留（FR-SEARCH-24 不被拆）、排除项仍是链式 NOT。
#[test]
fn inline_or_combines_phrases_and_keeps_exclusions() {
    let parsed = parse_query("\"季度报告\" OR 草稿 -废弃", MatchMode::All).expect("解析");
    assert!(
        parsed.match_expr.contains(&phrase_expr("季度报告")),
        "短语不得被拆：{}",
        parsed.match_expr
    );
    assert!(
        parsed.match_expr.contains(&escape_fts("草稿")),
        "普通词进正面集合：{}",
        parsed.match_expr
    );
    assert!(
        parsed.match_expr.contains(" NOT "),
        "排除项照旧链式 NOT：{}",
        parsed.match_expr
    );
    assert!(
        parsed.highlight.contains(&"季度报告".to_string()),
        "高亮词不含运算符：{:?}",
        parsed.highlight
    );
    assert!(
        !parsed.highlight.iter().any(|t| t == "OR"),
        "OR 不是高亮词：{:?}",
        parsed.highlight
    );
}

/// OR 关键字的边界：**只认大写**、**引号里不算**、单独一个 OR 是空查询而不是检索词。
///
/// 为什么只认大写：小写 `or` 是英文正文里的常见词（`this or that`），当运算符会让它搜不到；
/// 要搜字面量 `OR` 可以用引号短语。
#[test]
fn inline_or_is_uppercase_and_not_an_operator_inside_quotes() {
    // 小写 or 是普通检索词：AND 模式照旧连接它
    let lower = expr("a or b", MatchMode::All);
    assert!(lower.contains(" AND "), "小写 or 不是运算符：{lower}");
    assert!(
        lower.to_lowercase().contains("\"or\""),
        "它作为词进 MATCH：{lower}"
    );
    // 引号短语里的 OR 是字面量（短语整体不进运算符判断）
    let quoted = expr("\"OR\"", MatchMode::All);
    assert!(!quoted.contains(" OR "), "引号内 OR 不是运算符：{quoted}");
    // 只剩一个 OR ⇒ 摘掉后没有正面词：按「空查询」而不是检索词处理
    assert!(
        parse_query("OR", MatchMode::All)
            .expect("单独 OR 不是语法错误")
            .is_empty_query(),
        "只有 OR 时等价于没有查询内容"
    );
}

/// 排除项必须作用于**整个**正面集合：FTS5 里 NOT 优先级高于 AND/OR，
/// `a OR b NOT c` 会被解析成 `a OR (b NOT c)`（含 c 的 a 仍命中）⇒ 必须加括号。
#[test]
fn exclusions_apply_to_the_whole_positive_expression() {
    let e = expr("报告 OR 草稿 -废弃", MatchMode::All);
    assert!(
        e.starts_with("(\"") || e.starts_with('('),
        "排除项存在时正面表达式必须加括号：{e}"
    );
    assert!(e.contains(") NOT "), "NOT 必须作用于括号整体：{e}");
    // 无排除项时不加括号（不改变既有表达式形状）
    let plain = expr("报告 OR 草稿", MatchMode::All);
    assert!(!plain.starts_with('('), "没有排除项就不该加括号：{plain}");
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
