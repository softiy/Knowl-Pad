//! 全文搜索的**结果组装**（从 commands::search_index 拆出：CODE-11 的 200 SLOC 预警线）。
//!
//! 两条路径：
//! - **常规**：分页交给 SQL，total 与 items 出自同一 spec（IPC-05 的分页一致性）；
//! - **`line:(...)`**：先取**全部**候选读原文判「同一行」，再切片分页 —— 索引里没有换行信息
//!   （plain_text 是 jieba 分词后空格连接的，FTS-01），SQL 表达不了这条约束。
//!
//! 片段不进 SQL：FTS5 的 snippet() 作用在分词后的 plain_text 上（带空格、与原文不逐字一致），
//! 这里按技术方案 §5.2「高亮还原」的口径做：命中 → 读原文 → 原文定位 → 截窗口。

use std::path::Path;

use crate::commands::search_view::{snippet_at, SearchResultItem, SearchSnippet};
use crate::error_wrapper::KpError;
use crate::storage::pool::DbPool;
use crate::storage::search_query::{self, FtsHit, SortOrder};
use kp_domain::search::ParsedQuery;
use kp_domain::search_hits::{self, same_line, Hit};

/// 呈现参数（片段上下文行数 + 每条结果的片段数上限）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewOptions {
    pub context_lines: usize,
    pub max_snippets: usize,
}

/// 已夹紧的分页窗口。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageWindow {
    pub offset: i64,
    pub limit: i64,
}

/// 一页结果（条目 + 命中总数）。
pub(crate) struct PageResult {
    pub items: Vec<SearchResultItem>,
    pub total: i64,
}

/// 取前若干个片段：**同一行的多处命中共用一个片段**（否则同一段文字会重复出现 ——
/// FR-SEARCH-05 的「合并」精神）。
fn collect_snippets(content: &str, hits: &[Hit], view: ViewOptions) -> Vec<SearchSnippet> {
    let mut out: Vec<SearchSnippet> = Vec::new();
    let mut used_lines: Vec<u32> = Vec::new();
    for hit in hits {
        if out.len() >= view.max_snippets {
            break;
        }
        if used_lines.contains(&hit.line) {
            continue;
        }
        if let Some(snippet) = snippet_at(content, hit, view.context_lines) {
            used_lines.push(hit.line);
            out.push(snippet);
        }
    }
    out
}

/// 组装一条结果（正文由调用方给出：同行过滤路径已经读过一次盘，不再重复读）。
///
/// 读不到原文（索引与磁盘短暂不一致、文件被外部删除）时**降级**为 0 命中 + 空片段，
/// 而不是让整次搜索失败 —— 但必须留痕，否则「搜到了却看不到片段」无从排查。
pub(crate) fn item_of(
    hit: &FtsHit,
    content: Option<&str>,
    terms: &[String],
    view: ViewOptions,
) -> SearchResultItem {
    let (hit_count, snippets) = match content {
        Some(text) => {
            let hits = search_hits::locate(text, terms);
            (hits.len(), collect_snippets(text, &hits, view))
        }
        None => {
            tracing::debug!(rel_path = %hit.rel_path, "命中笔记读取失败，本条降级为空片段");
            (0, Vec::new())
        }
    };
    SearchResultItem {
        rel_path: hit.rel_path.clone(),
        name: hit.name.clone(),
        mtime_ms: hit.mtime_ms,
        score: hit.score,
        hit_count,
        snippets,
    }
}

/// 读原文后再组装（常规路径）。
fn build_item(root: &Path, hit: &FtsHit, terms: &[String], view: ViewOptions) -> SearchResultItem {
    // 复用反链面板那份「SEC-02 校验 + 读不到就降级」的实现，避免两套降级语义
    let content = crate::commands::link_view::read_note(root, &hit.rel_path);
    item_of(hit, content.as_deref(), terms, view)
}

/// 常规路径：分页由 SQL 完成。
pub(crate) fn page_without_line_filter(
    pool: &DbPool,
    root: &Path,
    parsed: &ParsedQuery,
    sort: SortOrder,
    view: ViewOptions,
    window: PageWindow,
) -> Result<PageResult, KpError> {
    let (rows, total) =
        search_query::fulltext_page(pool, parsed, sort, window.limit, window.offset)
            .map_err(KpError)?;
    let items = rows
        .iter()
        .map(|hit| build_item(root, hit, &parsed.highlight, view))
        .collect();
    Ok(PageResult { items, total })
}

/// `line:(...)` 路径：先对**全部候选**判「同一行」，再切片分页。
///
/// 为什么不能只过滤当前页：total 必须等于过滤后的命中数，否则 hasMore 与翻页会漏项/重复。
/// 代价：候选数 = 含全部 line: 词的笔记数，这批笔记会被读一遍。line: 是 P2 的窄化语法
/// （它本身就是在要求「几个词挤在同一行」），大 Vault 上的实测口径见本 PR 报告。
pub(crate) fn page_with_line_filter(
    pool: &DbPool,
    root: &Path,
    parsed: &ParsedQuery,
    sort: SortOrder,
    view: ViewOptions,
    window: PageWindow,
) -> Result<PageResult, KpError> {
    let rows = search_query::fulltext_scan(pool, parsed, sort).map_err(KpError)?;
    let mut passing: Vec<(&FtsHit, String)> = Vec::new();
    for hit in &rows {
        match crate::commands::link_view::read_note(root, &hit.rel_path) {
            Some(content) if same_line(&content, &parsed.filters.line_terms) => {
                passing.push((hit, content));
            }
            Some(_) => {}
            None => {
                tracing::debug!(rel_path = %hit.rel_path, "候选笔记读取失败，同行过滤按不通过处理");
            }
        }
    }
    let total = passing.len() as i64;
    let items = passing
        .into_iter()
        .skip(window.offset.max(0) as usize)
        .take(window.limit.max(0) as usize)
        .map(|(hit, content)| item_of(hit, Some(&content), &parsed.highlight, view))
        .collect();
    Ok(PageResult { items, total })
}
