//! 高级搜索语法的解析（FR-SEARCH-20~27；技术方案 §5.2.2 的 `extract_filters` / `extract_quoted`）。
//!
//! 本模块只做一件事：把用户输入切成「结构化过滤条件」与「全文关键词」，并把**语法错误**
//! 变成 `AppError::SearchSyntax`（FR-SEARCH-27 / AC-SEARCH-05：必须明确提示，
//! **不得**静默返回空结果——那会让用户以为"没有匹配"）。
//!
//! **为什么过滤条件不进 FTS5 的 MATCH 表达式**：`path:` / `file:` / `tag:` 约束的是
//! `file` / `tag` 表的列，而 MATCH 只能约束 `note_fts.plain_text`。把它们留在表达式里，
//! 「path:work 报告」会退化成在正文里搜 "path" 与 "work" 两个词 —— 这正是 FR-SEARCH-21
//! 「不参与全文」要排除的行为。过滤条件因此由 storage::search_query 变成 WHERE 谓词。
//!
//! **运算符一律小写且紧跟冒号**（`path:` / `file:` / `tag:` / `line:(`）：PRD 第 1090–1093 行
//! 给的就是这四个字面量，不做大小写放宽 —— 否则 `File: 报告` 这类正常检索词会被意外吞成过滤条件。
//!
//! **语法错误的文案里不含用户输入**：命令层的错误出口会把 `format!("{:?}", err)` 放进 IPC
//! 响应的 detail 字段，把原查询拼进消息就等于把搜索词搬到了错误通道上（SEC-09 / R-12 的同一纪律）。

use crate::error::AppError;

/// 双引号未闭合（AC-SEARCH-05 的原文场景：输入 `"未闭合的短语`）。
pub const UNCLOSED_QUOTE: &str = r#"双引号未闭合：短语需要用一对 " 包起来（如 "季度报告"）"#;
/// `line:(` 的括号未闭合。
pub const UNCLOSED_PAREN: &str = "line: 的括号未闭合：请写成 line:(词1 词2)";
/// 过滤条件只有键没有值。
pub const EMPTY_FILTER_VALUE: &str =
    "过滤条件缺少取值：path: / file: / tag: 后要跟内容，line: 的括号里至少要有一个词";
/// `line:` 没有紧跟括号（`line:` 或 `line: 词` 这类写法）。
pub const LINE_NEEDS_PAREN: &str = "line: 后面必须紧跟括号：请写成 line:(词1 词2)";

/// `line:` 的运算符前缀（含左括号，便于一次性判断并消费）。
const LINE_PREFIX: &str = "line:(";

/// 高级语法里的**结构化过滤条件**（从查询串里摘除，不再参与全文匹配）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchFilters {
    /// `path:folder/`（FR-SEARCH-20）：路径前缀，已归一化为 `folder`（无首尾 `/`、分隔符统一为 `/`）。
    pub path_prefix: Option<String>,
    /// `file:名称`（FR-SEARCH-21）：文件名子串；**不参与全文**。
    pub file_name: Option<String>,
    /// `tag:#标签`（FR-SEARCH-22）：规范化后的标签（小写、无 `#`）。层级前缀匹配由 storage 侧做，
    /// 因为 MD-TAG-03 已把 `#父/子` 同时登记到 `父`，两边都要照顾到。
    /// 多个 `tag:` 之间是 **AND**。
    pub tags: Vec<String>,
    /// `line:(...)`（FR-SEARCH-23）：必须出现在**同一行**的词（原文口径，见 `search_hits::same_line`）。
    pub line_terms: Vec<String>,
}

impl SearchFilters {
    /// 是否没有任何过滤条件（用于区分「空查询」与「纯过滤查询」）。
    pub fn is_empty(&self) -> bool {
        self.path_prefix.is_none()
            && self.file_name.is_none()
            && self.tags.is_empty()
            && self.line_terms.is_empty()
    }
}

/// 摘出高级语法过滤条件，返回（过滤条件, 剩余的全文关键词文本）。
///
/// 剩余的文本里**短语仍带引号**，供 `extract_quoted` 继续处理（两函数的分工与技术方案
/// §5.2.2 的 `extract_filters` / `extract_quoted` 一致）。
pub fn extract_filters(query: &str) -> Result<(SearchFilters, String), AppError> {
    let mut filters = SearchFilters::default();
    let mut rest: Vec<String> = Vec::new();
    let mut sc = Scanner::new(query);
    loop {
        sc.skip_ws();
        if sc.is_empty() {
            break;
        }
        if sc.peek() == Some('"') {
            // 引号短语必须整体保留（含引号）——否则短语会被后面的按空白切分拆开（FR-SEARCH-24）
            let phrase = sc.read_quoted()?.trim().to_string();
            if !phrase.is_empty() {
                rest.push(format!("\"{phrase}\""));
            }
            continue;
        }
        // 运算符**大小写不敏感**（PRD §5.3.4.3 的裁定）：Line:( 与 line:( 等价
        if sc.starts_with_ignore_ascii_case(LINE_PREFIX) {
            sc.advance(LINE_PREFIX.len());
            filters
                .line_terms
                .extend(split_group(&sc.read_paren_group()?)?);
            continue;
        }
        let word = sc.read_bare();
        if !absorb_prefixed(&word, &mut filters)? {
            rest.push(word);
        }
    }
    Ok((filters, rest.join(" ")))
}

/// 取出双引号短语（FR-SEARCH-24：**不拆词**），返回（短语, 其余文本）。
pub fn extract_quoted(text: &str) -> Result<(Vec<String>, String), AppError> {
    let mut phrases: Vec<String> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    let mut sc = Scanner::new(text);
    loop {
        sc.skip_ws();
        if sc.is_empty() {
            break;
        }
        if sc.peek() == Some('"') {
            let phrase = sc.read_quoted()?.trim().to_string();
            if !phrase.is_empty() {
                phrases.push(phrase);
            }
            continue;
        }
        rest.push(sc.read_bare());
    }
    Ok((phrases, rest.join(" ")))
}

/// 单个「键:值」词是否被识别为过滤条件；识别成功返回 true（该词不进全文）。
///
/// **运算符大小写不敏感**（PRD §5.3.4.3）：键先折叠成小写再比对，故 `Tag:#工作` 与 `tag:#工作` 等价。
/// 前缀仍必须**位于词首且紧跟冒号** —— `xTag:#工作` 的键是 `xTag`，不会被当成运算符。
fn absorb_prefixed(word: &str, filters: &mut SearchFilters) -> Result<bool, AppError> {
    let Some((key, value)) = word.split_once(':') else {
        return Ok(false);
    };
    match key.to_ascii_lowercase().as_str() {
        "path" => {
            // 空路径（path:/、path:）等价于「Vault 根」= 不加路径限制，**合法**（PRD §5.3.4.3）
            filters.path_prefix = normalize_path(value);
            Ok(true)
        }
        "file" => {
            filters.file_name = Some(require_value(value)?.to_string());
            Ok(true)
        }
        "tag" => {
            filters.tags.push(normalize_tag(value)?);
            Ok(true)
        }
        // 走到这里说明没有紧跟括号（带括号的写法在 extract_filters 里已被消费）：
        // `line:` / `line: 词` 都是残缺语法，必须明说而不是当普通检索词吞掉
        "line" => Err(AppError::SearchSyntax(LINE_NEEDS_PAREN.to_string())),
        _ => Ok(false),
    }
}

/// `path:` 的取值归一化：统一分隔符为 `/`（相对路径口径，R-09）、去掉首尾 `/`。
fn normalize_path(value: &str) -> Option<String> {
    let normalized = value.trim().replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `tag:` 的取值归一化：去 `#`、折叠空层级、转小写（MD-TAG-05：匹配按 `norm`，norm 是小写）。
fn normalize_tag(value: &str) -> Result<String, AppError> {
    let cleaned = value
        .trim()
        .trim_start_matches('#')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if cleaned.is_empty() {
        return Err(AppError::SearchSyntax(EMPTY_FILTER_VALUE.to_string()));
    }
    Ok(cleaned.to_lowercase())
}

/// 取值非空校验（`file:` 这种只有键没有值的写法必须报错，不能静默忽略）。
fn require_value(value: &str) -> Result<&str, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::SearchSyntax(EMPTY_FILTER_VALUE.to_string()));
    }
    Ok(trimmed)
}

/// 拆 `line:(...)` 括号里的词：按空白切分，允许词本身带引号（`line:("精确短语" 词)`）。
fn split_group(body: &str) -> Result<Vec<String>, AppError> {
    let (phrases, rest) = extract_quoted(body)?;
    let words: Vec<String> = phrases
        .into_iter()
        .chain(rest.split_whitespace().map(str::to_string))
        .collect();
    if words.is_empty() {
        return Err(AppError::SearchSyntax(EMPTY_FILTER_VALUE.to_string()));
    }
    Ok(words)
}

/// 词法扫描器：按空白切词，但**双引号内是一个整体**。
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }

    fn remaining(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn skip_ws(&mut self) {
        let rest = self.remaining();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn is_empty(&self) -> bool {
        self.remaining().is_empty()
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// ASCII 大小写不敏感的前缀判断（运算符大小写不敏感，PRD §5.3.4.3）。
    ///
    /// 用 `get(..n)` 而不是切片：`n` 落在一个多字节字符中间时切片会 panic，
    /// 而 `get` 返回 None（前缀更长，自然不匹配）。
    fn starts_with_ignore_ascii_case(&self, prefix: &str) -> bool {
        self.remaining()
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    }

    fn advance(&mut self, bytes: usize) {
        self.pos += bytes;
    }

    /// 读到下一个空白（不含）或串尾。
    fn read_bare(&mut self) -> String {
        let rest = self.remaining();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        self.pos += end;
        rest[..end].to_string()
    }

    /// 读一个双引号短语（调用时 `pos` 必须停在开引号上）；未闭合即语法错误。
    fn read_quoted(&mut self) -> Result<String, AppError> {
        self.advance(1);
        let rest = self.remaining();
        let Some(end) = rest.find('"') else {
            return Err(AppError::SearchSyntax(UNCLOSED_QUOTE.to_string()));
        };
        let phrase = rest[..end].to_string();
        self.pos += end + 1;
        Ok(phrase)
    }

    /// 读 `line:(` 之后到右括号之间的内容（调用时 `pos` 已在左括号之后）。
    fn read_paren_group(&mut self) -> Result<String, AppError> {
        let rest = self.remaining();
        let Some(end) = rest.find(')') else {
            return Err(AppError::SearchSyntax(UNCLOSED_PAREN.to_string()));
        };
        let body = rest[..end].to_string();
        self.pos += end + 1;
        Ok(body)
    }
}

#[cfg(test)]
#[path = "search_syntax_tests.rs"]
mod tests;
