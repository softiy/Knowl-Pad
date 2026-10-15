//! 全文搜索与快速打开的命令体（PRD §5.3.4 的 search_fulltext / search_quick_open）。
//!
//! **搜索历史的写入路径（M5 批次 1 的口径选择，已在 PRD §5.3.4 登记）**：真源里**没有**
//! search_history_add 命令（只有 get / clear），而 FR-SEARCH-10 要求「保留最近 20 条查询」。
//! 因此记入动作收敛到 search_fulltext 的 options.recordHistory（**缺省 false**）：
//! ① 缺省 false 让这个**只读**命令保持无副作用 —— PRD §5.3 的约定是「写操作才标 ⚠️ 并审计」，
//!    而 search_fulltext 没有 ⚠️；
//! ② FR-SEARCH-08 的 200ms 防抖意味着「输入即查询」，每次调用都记会让历史塞满 知 / 知识 /
//!    知识图 这类前缀；「用户确认了一次查询」只有前端知道，故由前端在回车/点开结果时置 true。
//!
//! 本文件只做「解析 → 选查询路径 → 交给 search_page 组装」；片段与分页在 commands::search_page，
//! 高级语法的解析在 kp_domain::search_syntax + kp_domain::search。

use crate::commands::paths::root_of;
use crate::commands::search::{
    clamp_i64, clamp_usize, record_history_best_effort, PageArg, QuickOpenArgs, SearchFulltextArgs,
    SearchOptions, DEFAULT_MAX_SNIPPETS, DEFAULT_PAGE_LIMIT, DEFAULT_QUICK_OPEN_LIMIT,
    MAX_CONTEXT_LINES, MAX_LINE_FILTER_CANDIDATES, MAX_PAGE_LIMIT, MAX_QUICK_OPEN_LIMIT,
    MAX_SNIPPETS,
};
use crate::commands::search_page::{
    page_with_line_filter, page_without_line_filter, PageWindow, ViewOptions,
};
use crate::commands::search_view::{QuickOpenItem, SearchResultPage};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use crate::storage::search_query::{self, SortOrder};
use kp_domain::error::AppError;
use kp_domain::search::{parse_query, MatchMode};
use kp_domain::{quick_open, snippet};

fn resolve_page(arg: PageArg) -> PageWindow {
    PageWindow {
        // 负数 offset 没有意义（SQLite 会当 0，但显式夹紧更诚实）
        offset: arg.offset.unwrap_or(0).max(0),
        limit: clamp_i64(arg.limit, DEFAULT_PAGE_LIMIT, 1, MAX_PAGE_LIMIT),
    }
}

fn empty_page(page: PageWindow) -> SearchResultPage {
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

/// 片段与条数上限（IPC-05 的载荷纪律：夹紧而不是报错）。
fn view_options(options: &SearchOptions) -> ViewOptions {
    ViewOptions {
        context_lines: clamp_usize(
            options.context_lines,
            snippet::DEFAULT_CONTEXT_LINES,
            0,
            MAX_CONTEXT_LINES,
        ),
        max_snippets: clamp_usize(options.max_snippets, DEFAULT_MAX_SNIPPETS, 1, MAX_SNIPPETS),
    }
}

/// 全文搜索（FR-SEARCH-01~09；高级语法 FR-SEARCH-20~27）。
pub(crate) fn search_fulltext_impl(
    state: &AppState,
    args: SearchFulltextArgs,
) -> Result<SearchResultPage, KpError> {
    let pool = state.index_db().ok_or(KpError(AppError::VaultNotOpen))?;
    let root = root_of(state)?;
    let page = resolve_page(args.page);
    // 语法错误（未闭合引号等）在这里变成 E_SEARCH_SYNTAX，**不会**被当成「没有结果」（FR-SEARCH-27）
    let parsed = parse_query(&args.query, match_mode(&args.options)).map_err(KpError)?;
    if parsed.is_empty_query() {
        // 空查询 = 没有查询（kp_domain::search 的约定），返回空页而**不是**语法错误；
        // 纯过滤查询（如 tag:#工作）不走这里 —— 它必须真的去列笔记
        return Ok(empty_page(page));
    }
    let sort = sort_order(&args.options);
    let view = view_options(&args.options);
    let started = std::time::Instant::now();
    let result = if parsed.filters.line_terms.is_empty() {
        page_without_line_filter(&pool, &root, &parsed, sort, view, page)?
    } else {
        // line: 必须扫全部候选（见 search_page 的说明），因此带上候选数上限；
        // 超限时这里会返回 E_SEARCH_SYNTAX + 「请先缩小范围」的提示（PRD §5.3.4.3）
        page_with_line_filter(
            &pool,
            &root,
            &parsed,
            sort,
            view,
            page,
            MAX_LINE_FILTER_CANDIDATES,
        )?
    };
    if args.options.record_history.unwrap_or(false) {
        record_history_best_effort(state, &args.query);
    }
    // NFR-PERF-04 / FR-SEARCH-09 的耗时只进日志（**不含查询词**，SEC-09 / R-12）
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        total = result.total,
        "全文搜索完成"
    );
    Ok(SearchResultPage {
        items: result.items,
        total: result.total,
        offset: page.offset,
        limit: page.limit,
        has_more: page.offset + page.limit < result.total,
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
