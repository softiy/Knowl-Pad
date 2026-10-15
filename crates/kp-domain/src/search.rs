//! FTS5 查询构造（技术方案 §5.2 查询侧；PRD FTS-02）。
//!
//! 本层把用户输入变成「合法的 MATCH 表达式 + 结构化过滤条件」：
//! - token 一律用双引号包裹（escape_fts），避免 " * ^ : ( ) - + 引发语法错误或意外语义；
//! - 双引号短语**不拆词**（FR-SEARCH-24 精确短语）；
//! - `-词` 为排除项（FR-SEARCH-25），且 **FTS5 的 NOT 是二元运算符** → 多个排除词链式 expr NOT a NOT b；
//! - `path:` / `file:` / `tag:` / `line:`（FR-SEARCH-20~23）由 `search_syntax` 摘成过滤条件，
//!   **不进** MATCH —— 见该模块顶部「为什么过滤条件不进 MATCH」；
//! - 语法错误（未闭合引号等）一律 `AppError::SearchSyntax`（FR-SEARCH-27 / AC-SEARCH-05）；
//! - 其余词按 ALL（AND，默认）或 ANY（OR）连接。

use crate::error::AppError;
use crate::search_syntax::{self, SearchFilters};
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

    fn joiner(self) -> &'static str {
        match self {
            Self::All => " AND ",
            Self::Any => " OR ",
        }
    }
}

/// FTS5 转义：双引号包裹并把内部双引号翻倍（TECH:1557）。
pub fn escape_fts(token: &str) -> String {
    format!("\"{}\"", token.replace('"', "\"\""))
}

/// 短语的 MATCH 写法（FR-SEARCH-24：**不拆词**）。
///
/// **必须先按 jieba 切开、再用空格连接、整体放进双引号**：
/// 索引里的 plain_text 就是 jieba 分词后用空格连接的（FTS-01），而 FTS5 的 unicode61
/// 会把一整串汉字当成**一个** token —— 直接发 \"季度报告\" 是一个永远匹配不到的 token。
/// 2026-10-15 探针实测（真实索引）：\"季度报告\" → **0 命中**；\"季度 报告\" → 1 命中
/// （\"季度和报告\" 不命中）。切成 token 后作为 FTS5 **短语查询**发出，既要求这些词相邻且
/// 有序出现（用户给的整体序列不被拆散），又能真正命中索引。
fn phrase_expr(phrase: &str) -> String {
    let tokens = tokenize::for_query(phrase);
    if tokens.is_empty() {
        // 纯标点短语：没有 token 可切，保持原文交给 FTS5（匹配不到，但**不会**变成语法错误）
        return escape_fts(phrase);
    }
    format!("\"{}\"", tokens.join(" "))
}

/// 一次查询的全部解析产物（**唯一真相**：MATCH 构造、过滤条件、原文高亮都出自这里，
/// 避免三处各自解析导致「搜到的」与「高亮的」对不上）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// 完整 MATCH 表达式（正面词 + 链式 NOT）。空串表示**一个全文词都没有**。
    pub match_expr: String,
    /// 只含正面词的表达式片段。空串 = 这次查询没有全文正面词 —— storage 据此走「纯过滤查询」
    /// （没有正面词时 FTS5 缺左侧操作数，排除项只能改写成 NOT IN 子查询）。
    pub positive_expr: String,
    /// 分词后的排除词（FR-SEARCH-25），供上面的 NOT IN 改写使用。
    pub excludes: Vec<String>,
    /// 高级语法过滤条件（FR-SEARCH-20~23）。
    pub filters: SearchFilters,
    /// 原文高亮词（FR-SEARCH-04）：短语整段 + 分词后的正面词；**不含**排除词与过滤条件。
    pub highlight: Vec<String>,
}

impl ParsedQuery {
    /// 是否是一次「什么都没有」的查询（空串/纯空白）：既没有全文词也没有过滤条件。
    ///
    /// 与「纯过滤查询」（如 `tag:#工作`）区分开 —— 前者返回空页，后者必须真的去列笔记。
    pub fn is_empty_query(&self) -> bool {
        self.match_expr.is_empty() && self.filters.is_empty()
    }
}

/// 解析查询：过滤条件 → 短语 → 排除项 → 分词（技术方案 §5.2.2 的查询侧步骤 1~4）。
pub fn parse_query(query: &str, mode: MatchMode) -> Result<ParsedQuery, AppError> {
    // 1) 高级语法前缀（path: file: tag: line:）；语法错误 → E_SEARCH_SYNTAX
    let (filters, text) = search_syntax::extract_filters(query)?;
    // 2) 双引号短语（不拆词）；未闭合引号 → E_SEARCH_SYNTAX
    let (phrases, rest) = search_syntax::extract_quoted(&text)?;
    // 3) 排除项（- 前缀）
    let (exclude_text, include_text) = split_negation(&rest);
    // 4) 分词
    let includes = tokenize::for_query(&include_text);
    let excludes = tokenize::for_query(&exclude_text);
    // line: 组里的词既是过滤条件（必须同一行），也是必须出现的全文正面词
    let line_tokens: Vec<String> = filters
        .line_terms
        .iter()
        .flat_map(|term| tokenize::for_query(term))
        .collect();

    let mut positives: Vec<String> = phrases.iter().map(|p| phrase_expr(p)).collect();
    positives.extend(includes.iter().map(|t| escape_fts(t)));
    positives.extend(line_tokens.iter().map(|t| escape_fts(t)));
    let positive_expr = positives.join(mode.joiner());
    // 5) 构造 MATCH：只有排除词时 FTS5 缺左侧操作数，退回这些词的 OR（M3 起的既有兜底语义）
    let match_expr = if positive_expr.is_empty() {
        excludes
            .iter()
            .map(|t| escape_fts(t))
            .collect::<Vec<_>>()
            .join(" OR ")
    } else {
        excludes.iter().fold(positive_expr.clone(), |acc, term| {
            format!("{acc} NOT {}", escape_fts(term))
        })
    };

    Ok(ParsedQuery {
        match_expr,
        positive_expr,
        excludes,
        highlight: merge_highlight(&phrases, &includes, &line_tokens),
        filters,
    })
}

/// 把剩余文本按空白切开：`-词` 进排除表，其余进正面词。
///
/// 单独的 `-` 两个表都不进（它没有可排除的内容）。
fn split_negation(rest: &str) -> (String, String) {
    let mut excludes: Vec<&str> = Vec::new();
    let mut includes: Vec<&str> = Vec::new();
    for raw in rest.split_whitespace() {
        match raw.strip_prefix('-') {
            Some(term) if !term.is_empty() => excludes.push(term),
            Some(_) => {}
            None => includes.push(raw),
        }
    }
    (excludes.join(" "), includes.join(" "))
}

/// 高亮词表去重（顺序即优先级：短语在词之前）。
fn merge_highlight(phrases: &[String], includes: &[String], line_tokens: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for term in phrases.iter().chain(includes).chain(line_tokens) {
        if !out.contains(term) {
            out.push(term.clone());
        }
    }
    out
}

/// 构造 MATCH 表达式（函数名与技术方案 §5.2.2 一致）。
///
/// 空查询返回空串（调用方应视为「无查询」而非语法错误）；**语法错误**才返回 `Err`。
pub fn build_match_expr(query: &str, mode: MatchMode) -> Result<String, AppError> {
    parse_query(query, mode).map(|parsed| parsed.match_expr)
}

/// 原文高亮的词表（FR-SEARCH-04）。
///
/// **只取正面词**（短语 + 分词结果 + line: 组内词）：用户写 `-草稿` 是想「不要这些结果」，
/// 而不是想在结果里看到「草稿」被点亮。
pub fn highlight_terms(query: &str) -> Result<Vec<String>, AppError> {
    parse_query(query, MatchMode::All).map(|parsed| parsed.highlight)
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
