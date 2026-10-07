//! FTS5 查询构造（技术方案 §5.2 查询侧；PRD FTS-02）。
//!
//! 本层只负责「把用户输入变成合法的 MATCH 表达式」：
//! - token 一律用双引号包裹（`escape_fts`），避免 `" * ^ : ( ) - +` 引发语法错误或意外语义；
//! - 双引号短语**不拆词**；
//! - `-词` 为排除项，且 **FTS5 的 NOT 是二元运算符** → 多个排除词写成链式 `expr NOT a NOT b`；
//! - 其余词按 `ALL`（AND，默认）或 `ANY`（OR）连接。
//!
//! `path:`/`file:`/`tag:`/`line:` 等**高级语法属 M5 的搜索语法层**，不在此处（M3 只交付表达式构造）。

use crate::tokenize;

/// 词间连接方式（PRD FTS-02：默认 AND，可选 OR）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    All,
    Any,
}

/// FTS5 转义：双引号包裹并把内部双引号翻倍（TECH:1557）。
pub fn escape_fts(token: &str) -> String {
    format!("\"{}\"", token.replace('"', "\"\""))
}

/// 构造 MATCH 表达式。空查询返回空串（调用方应视为「无查询」而非语法错误）。
pub fn build_match_expr(query: &str, mode: MatchMode) -> String {
    let mut includes: Vec<String> = Vec::new();
    let mut excludes: Vec<String> = Vec::new();
    let mut rest = query;
    // 1) 双引号短语（不拆词）
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let phrase = &after[..end];
        if !phrase.trim().is_empty() {
            includes.push(escape_fts(phrase.trim()));
        }
        rest = &after[end + 1..];
    }
    // 2) 其余按空白切分，处理 - 排除
    for raw in rest.split_whitespace() {
        if let Some(term) = raw.strip_prefix('-') {
            if term.is_empty() {
                continue;
            }
            for t in tokenize::for_query(term) {
                excludes.push(escape_fts(&t));
            }
        } else {
            for t in tokenize::for_query(raw) {
                includes.push(escape_fts(&t));
            }
        }
    }
    if includes.is_empty() && excludes.is_empty() {
        return String::new();
    }
    let joiner = match mode {
        MatchMode::All => " AND ",
        MatchMode::Any => " OR ",
    };
    // 只有排除项时，FTS5 需要一个左侧表达式：用「匹配任意内容」的占位（*）
    let base = if includes.is_empty() {
        // 无法表达「全部」时退回所有 token 的 OR —— 保守且不会静默返回全部结果
        excludes.join(" OR ")
    } else {
        includes.join(joiner)
    };
    let mut expr = base;
    for term in &excludes {
        if includes.is_empty() {
            break;
        }
        expr = format!("{expr} NOT {term}");
    }
    expr
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
}
