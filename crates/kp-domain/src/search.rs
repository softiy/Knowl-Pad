//! FTS5 查询构造（技术方案 §5.2 查询侧；PRD FTS-02）。
//!
//! 本层只负责「把用户输入变成合法的 MATCH 表达式」：
//! - token 一律用双引号包裹（escape_fts），避免 " * ^ : ( ) - + 引发语法错误或意外语义；
//! - 双引号短语**不拆词**；
//! - -词 为排除项，且 **FTS5 的 NOT 是二元运算符** → 多个排除词写成链式 expr NOT a NOT b；
//! - 其余词按 ALL（AND，默认）或 ANY（OR）连接。
//!
//! path: / file: / tag: / line: 等**高级语法属 M5 的搜索语法层**，不在此处（M3 只交付表达式构造，
//! 本 PR 仍不含高级语法 —— 见 PR 报告的「未做」一节）。

use crate::tokenize;

/// 词间连接方式（PRD FTS-02：默认 AND，可选 OR）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    All,
    Any,
}

impl MatchMode {
    /// 线上表示（PRD §5.3.4 的 options.matchMode）：all / any；未知取 None，由调用方决定默认值。
    pub fn parse(wire: &str) -> Option<Self> {
        match wire {
            "all" => Some(Self::All),
            "any" => Some(Self::Any),
            _ => None,
        }
    }
}

/// FTS5 转义：双引号包裹并把内部双引号翻倍（TECH:1557）。
pub fn escape_fts(token: &str) -> String {
    format!("\"{}\"", token.replace('"', "\"\""))
}

/// 查询串的解析结果（**唯一真相**：MATCH 构造与原文高亮都走这里，避免两处解析对不上）。
#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedQuery {
    /// 双引号短语（不拆词）
    phrases: Vec<String>,
    /// 正面词（已分词）
    includes: Vec<String>,
    /// 排除词（已分词）
    excludes: Vec<String>,
}

fn parse_query(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut rest = query;
    // 1) 双引号短语（不拆词）
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let phrase = &after[..end];
        if !phrase.trim().is_empty() {
            parsed.phrases.push(phrase.trim().to_string());
        }
        rest = &after[end + 1..];
    }
    // 2) 其余按空白切分，处理 - 排除
    for raw in rest.split_whitespace() {
        if let Some(term) = raw.strip_prefix('-') {
            if term.is_empty() {
                continue;
            }
            parsed.excludes.extend(tokenize::for_query(term));
        } else {
            parsed.includes.extend(tokenize::for_query(raw));
        }
    }
    parsed
}

/// 构造 MATCH 表达式。空查询返回空串（调用方应视为「无查询」而非语法错误）。
pub fn build_match_expr(query: &str, mode: MatchMode) -> String {
    let parsed = parse_query(query);
    if parsed.phrases.is_empty() && parsed.includes.is_empty() && parsed.excludes.is_empty() {
        return String::new();
    }
    let joiner = match mode {
        MatchMode::All => " AND ",
        MatchMode::Any => " OR ",
    };
    let mut positives: Vec<String> = parsed.phrases.iter().map(|p| escape_fts(p)).collect();
    positives.extend(parsed.includes.iter().map(|t| escape_fts(t)));
    // 只有排除项时，FTS5 需要一个左侧表达式：退回这些词的 OR（保守，且不会静默返回全部结果）
    if positives.is_empty() {
        return parsed
            .excludes
            .iter()
            .map(|t| escape_fts(t))
            .collect::<Vec<_>>()
            .join(" OR ");
    }
    let mut expr = positives.join(joiner);
    for term in &parsed.excludes {
        expr = format!("{expr} NOT {}", escape_fts(term));
    }
    expr
}

/// 原文高亮的词表（FR-SEARCH-04）：**只取正面词**（短语 + 分词结果）。
///
/// 排除词不参与高亮与命中计数 —— 用户写 -草稿 是想「不要这些结果」，
/// 而不是想在结果里看到「草稿」被点亮。
pub fn highlight_terms(query: &str) -> Vec<String> {
    let parsed = parse_query(query);
    let mut out: Vec<String> = Vec::with_capacity(parsed.phrases.len() + parsed.includes.len());
    for term in parsed.phrases.into_iter().chain(parsed.includes) {
        if !out.contains(&term) {
            out.push(term);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_wraps_in_quotes_and_doubles_inner() {
        assert_eq!(escape_fts("a\"b"), "\"a\"\"b\"");
        assert_eq!(escape_fts("a:b*c"), "\"a:b*c\"");
    }

    #[test]
    fn phrase_is_not_split() {
        let e = build_match_expr("\"季度报告\"", MatchMode::All);
        assert_eq!(e, "\"季度报告\"", "双引号短语不拆词");
    }

    #[test]
    fn all_mode_uses_and_any_mode_uses_or() {
        let a = build_match_expr("markdown test", MatchMode::All);
        assert!(a.contains(" AND "), "默认 AND：{a}");
        let o = build_match_expr("markdown test", MatchMode::Any);
        assert!(o.contains(" OR "), "可选 OR：{o}");
    }

    #[test]
    fn exclusions_use_chained_not() {
        let e = build_match_expr("报告 -草稿 -废弃", MatchMode::All);
        assert!(e.contains("NOT"), "应有排除：{e}");
        assert_eq!(e.matches("NOT").count(), 2, "多个排除词要链式 NOT：{e}");
        assert!(e.contains("草稿") && e.contains("废弃"));
    }

    #[test]
    fn empty_query_yields_empty_expr() {
        assert_eq!(build_match_expr("   ", MatchMode::All), "");
    }

    #[test]
    fn special_chars_do_not_break_syntax() {
        // 用户输入含 FTS5 元字符时不得产生语法错误（由 escape 保证）
        let e = build_match_expr("a*b (c) ^d", MatchMode::All);
        assert!(e.starts_with('"') || e.is_empty(), "表达式：{e}");
        assert!(!e.contains("(c)"), "括号必须被引号包裹：{e}");
    }

    #[test]
    fn only_excludes_still_yields_a_usable_expr() {
        let e = build_match_expr("-草稿", MatchMode::All);
        assert!(
            !e.is_empty(),
            "不能返回空表达式（那会静默变成「无查询」）：{e}"
        );
        assert!(!e.contains("NOT"), "没有左侧操作数时不能用 NOT：{e}");
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
        let terms = highlight_terms("\"季度报告\" 工作 -草稿");
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
        let terms = highlight_terms("图谱 图谱 图谱");
        assert_eq!(terms, vec!["图谱".to_string()], "重复词只留一份");
    }

    #[test]
    fn highlight_terms_for_empty_query_is_empty() {
        assert!(highlight_terms("   ").is_empty());
        assert!(
            highlight_terms("-草稿").is_empty(),
            "只有排除词时没有可高亮的正面词"
        );
    }
}
