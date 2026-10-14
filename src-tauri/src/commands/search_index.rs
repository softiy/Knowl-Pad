//! 全文搜索与快速打开的命令体（PRD §5.3.4 的 search_fulltext / search_quick_open）。
//!
//! **搜索历史的写入路径（本 PR 的口径选择，已在报告里登记待裁定）**：真源里**没有**
//! search_history_add 命令（只有 get / clear），而 FR-SEARCH-10 要求「保留最近 20 条查询」。
//! 因此记入动作收敛到 search_fulltext 的 options.recordHistory（**缺省 false**）：
//! ① 缺省 false 让这个**只读**命令保持无副作用 —— PRD §5.3 的约定是「写操作才标 ⚠️ 并审计」，
//!    而 search_fulltext 没有 ⚠️；
//! ② FR-SEARCH-08 的 200ms 防抖意味着「输入即查询」，每次调用都记会让历史塞满 知 / 知识 /
//!    知识图 这类前缀；「用户确认了一次查询」只有前端知道，故由前端在回车/点开结果时置 true。
//!
//! 片段不进 SQL：FTS5 的 snippet() 作用在分词后的 plain_text 上（带空格、与原文不逐字一致），
//! 这里按技术方案 §5.2「高亮还原」的口径做：命中 → 读原文 → 原文定位 → 截窗口。

use std::path::Path;

use crate::commands::paths::root_of;
use crate::commands::search::{
    clamp_i64, clamp_usize, record_history_best_effort, PageArg, QuickOpenArgs, SearchFulltextArgs,
    SearchOptions, DEFAULT_MAX_SNIPPETS, DEFAULT_PAGE_LIMIT, DEFAULT_QUICK_OPEN_LIMIT,
    MAX_CONTEXT_LINES, MAX_PAGE_LIMIT, MAX_QUICK_OPEN_LIMIT, MAX_SNIPPETS,
};
use crate::commands::search_view::{
    snippet_at, QuickOpenItem, SearchResultItem, SearchResultPage, SearchSnippet,
};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use crate::storage::search_query::{self, FtsHit, SortOrder};
use kp_domain::error::AppError;
use kp_domain::search::{build_match_expr, highlight_terms, MatchMode};
use kp_domain::search_hits::{self, Hit};
use kp_domain::{quick_open, snippet};

/// 夹紧后的分页参数。
struct Page {
    offset: i64,
    limit: i64,
}

fn resolve_page(arg: PageArg) -> Page {
    Page {
        // 负数 offset 没有意义（SQLite 会当 0，但显式夹紧更诚实）
        offset: arg.offset.unwrap_or(0).max(0),
        limit: clamp_i64(arg.limit, DEFAULT_PAGE_LIMIT, 1, MAX_PAGE_LIMIT),
    }
}

fn empty_page(page: Page) -> SearchResultPage {
    SearchResultPage {
        items: Vec::new(),
        total: 0,
        offset: page.offset,
        limit: page.limit,
        has_more: false,
    }
}

/// 缺省 all（PRD §4.11.3「默认匹配模式 AND」）；未知取值同样退回缺省而不是报错。
fn match_mode(options: &SearchOptions) -> MatchMode {
    options
        .match_mode
        .as_deref()
        .and_then(MatchMode::parse)
        .unwrap_or(MatchMode::All)
}

/// 缺省 relevance（FR-SEARCH-06「默认按相关度」）。
fn sort_order(options: &SearchOptions) -> SortOrder {
    options
        .sort
        .as_deref()
        .and_then(SortOrder::parse)
        .unwrap_or(SortOrder::Relevance)
}

/// 取前若干个片段：**同一行的多处命中共用一个片段**（否则同一段文字会重复出现 ——
/// FR-SEARCH-05 的「合并」精神）。
fn collect_snippets(
    content: &str,
    hits: &[Hit],
    context_lines: usize,
    max_snippets: usize,
) -> Vec<SearchSnippet> {
    let mut out: Vec<SearchSnippet> = Vec::new();
    let mut used_lines: Vec<u32> = Vec::new();
    for hit in hits {
        if out.len() >= max_snippets {
            break;
        }
        if used_lines.contains(&hit.line) {
            continue;
        }
        if let Some(view) = snippet_at(content, hit, context_lines) {
            used_lines.push(hit.line);
            out.push(view);
        }
    }
    out
}

/// 组装一条结果（FR-SEARCH-04/05）。
///
/// 读不到原文（索引与磁盘短暂不一致、文件被外部删除）时**降级**为 0 命中 + 空片段，
/// 而不是让整次搜索失败 —— 但必须留痕，否则「搜到了却看不到片段」无从排查。
fn build_item(
    root: &Path,
    hit: &FtsHit,
    terms: &[String],
    context_lines: usize,
    max_snippets: usize,
) -> SearchResultItem {
    // 复用反链面板那份「SEC-02 校验 + 读不到就降级」的实现，避免两套降级语义
    let content = crate::commands::link_view::read_note(root, &hit.rel_path);
    let (hit_count, snippets) = match content.as_deref() {
        Some(text) => {
            let hits = search_hits::locate(text, terms);
            let snippets = collect_snippets(text, &hits, context_lines, max_snippets);
            (hits.len(), snippets)
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

/// 全文搜索（FR-SEARCH-01~09）。
pub(crate) fn search_fulltext_impl(
    state: &AppState,
    args: SearchFulltextArgs,
) -> Result<SearchResultPage, KpError> {
    let pool = state.index_db().ok_or(KpError(AppError::VaultNotOpen))?;
    let root = root_of(state)?;
    let page = resolve_page(args.page);
    let expr = build_match_expr(&args.query, match_mode(&args.options));
    if expr.is_empty() {
        // 空查询 = 没有查询（kp_domain::search 的约定），返回空页而**不是**语法错误
        return Ok(empty_page(page));
    }
    let started = std::time::Instant::now();
    let (rows, total) = search_query::fulltext_page(
        &pool,
        &expr,
        sort_order(&args.options),
        page.limit,
        page.offset,
    )
    .map_err(KpError)?;
    let terms = highlight_terms(&args.query);
    let context_lines = clamp_usize(
        args.options.context_lines,
        snippet::DEFAULT_CONTEXT_LINES,
        0,
        MAX_CONTEXT_LINES,
    );
    let max_snippets = clamp_usize(
        args.options.max_snippets,
        DEFAULT_MAX_SNIPPETS,
        1,
        MAX_SNIPPETS,
    );
    let items = rows
        .iter()
        .map(|hit| build_item(&root, hit, &terms, context_lines, max_snippets))
        .collect();
    if args.options.record_history.unwrap_or(false) {
        record_history_best_effort(state, &args.query);
    }
    // NFR-PERF-04 / FR-SEARCH-09 的耗时只进日志（**不含查询词**，SEC-09 / R-12）
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        total,
        "全文搜索完成"
    );
    Ok(SearchResultPage {
        items,
        total,
        offset: page.offset,
        limit: page.limit,
        has_more: page.offset + page.limit < total,
    })
}

/// 快速打开（FR-SEARCH-40/41）：**只按文件名/路径匹配，不搜正文**。
pub(crate) fn search_quick_open_impl(
    state: &AppState,
    args: QuickOpenArgs,
) -> Result<Vec<QuickOpenItem>, KpError> {
    let pool = state.index_db().ok_or(KpError(AppError::VaultNotOpen))?;
    let limit = clamp_usize(
        args.limit,
        DEFAULT_QUICK_OPEN_LIMIT,
        1,
        MAX_QUICK_OPEN_LIMIT,
    );
    let candidates = search_query::quick_open_candidates(&pool).map_err(KpError)?;
    // 打分文本取 rel_path：FR-SEARCH-41 要求 work/rep 命中 work/2026/report.md，
    // 文件名本就是路径的后半段，因此一条字符串同时覆盖「文件名」与「路径片段」两种输入。
    let texts: Vec<String> = candidates.iter().map(|(rel, _)| rel.clone()).collect();
    Ok(quick_open::rank(&texts, &args.query, limit)
        .into_iter()
        .map(|(idx, score)| QuickOpenItem {
            rel_path: candidates[idx].0.clone(),
            name: candidates[idx].1.clone(),
            score,
        })
        .collect())
}
