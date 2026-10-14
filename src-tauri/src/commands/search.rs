//! 搜索域命令（PRD §5.3.4 的 **6 个命令**；名字与入参一律以该表第 1648–1653 行为准）。
//!
//! **⚠️ 的含义**（PRD §5.3 第 1583 行原文）：标注 ⚠️ 的为**写操作**，需施加额外的确认与审计（见 SEC-05）。
//! 本域有 2 个 —— replace_in_file 与 search_history_clear：确认在 UI 侧，**审计在这里**
//! （CMD-06：写命令入口记 INFO；SEC-09 / R-12 明令日志**不得**出现搜索词与正文）。
//!
//! **本域没有 search_history_add**：真源（PRD §5.3.4 与 M3 计划的「搜索域 6 命令」清单）里
//! 都不存在这个命令名，因此不新增 IPC 命令（那会让命令集与 PRD 的统计核对对不上）；
//! 历史写入路径见 search_index.rs 顶部的口径说明。
//!
//! 结构：命令体（*_impl，可直接用真实临时库单测，DEBT-14 的做法）在
//! search_index.rs（索引侧）/ search_file.rs（文件侧）；形状在 search_view.rs。

use serde::Deserialize;
use std::sync::Arc;
use tauri::State;

use crate::commands::search_file::{replace_in_file_impl, search_in_file_impl};
use crate::commands::search_index::{search_fulltext_impl, search_quick_open_impl};
use crate::commands::search_view::{InFileMatch, QuickOpenItem, ReplaceResult, SearchResultPage};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use crate::storage::pool::DbPool;
use kp_domain::error::AppError;

// ── 缺省值与上限（IPC-05：单响应 ≤ 4MB；夹紧而不是报错 —— 前端传错不该让搜索失败）────
/// 每页条数缺省（20 条 × 最多 20 个片段，远低于 4MB）。
pub const DEFAULT_PAGE_LIMIT: i64 = 20;
/// 每页条数上限。
pub const MAX_PAGE_LIMIT: i64 = 100;
/// 快速打开条数缺省（FR-SEARCH-40 的候选列表首屏）。
pub const DEFAULT_QUICK_OPEN_LIMIT: usize = 20;
/// 快速打开条数上限。
pub const MAX_QUICK_OPEN_LIMIT: usize = 100;
/// 每条结果的片段数缺省（FR-SEARCH-05「显示命中次数与首个片段（可展开查看全部片段）」）。
pub const DEFAULT_MAX_SNIPPETS: usize = 5;
/// 每条结果的片段数上限。
pub const MAX_SNIPPETS: usize = 20;
/// 片段上下文行数上限（PRD §4.11.3 的缺省值是 2；给 10^9 会让整篇笔记进响应）。
pub const MAX_CONTEXT_LINES: usize = 20;

/// 全文搜索选项（PRD §5.3.4 的 options）。字段逐个注明需求出处。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    /// 词间关系：all（缺省，PRD §4.11.3「默认匹配模式 AND」）/ any
    pub match_mode: Option<String>,
    /// 排序：relevance（缺省，FR-SEARCH-06）/ mtime / name / path
    pub sort: Option<String>,
    /// 片段上下文行数（PRD §4.11.3「片段上下文行数 2」）
    pub context_lines: Option<usize>,
    /// 每条结果最多返回几个片段（IPC-05 的载荷控制；FR-SEARCH-05 的「查看全部片段」）
    pub max_snippets: Option<usize>,
    /// 是否把本次查询记入历史（FR-SEARCH-10；**缺省 false**，理由见 search_index.rs）
    pub record_history: Option<bool>,
}

/// 分页（PRD §5.3.4 的 page）；offset/limit 缺省为 0 / DEFAULT_PAGE_LIMIT。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageArg {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

/// search_fulltext 入参（CMD-02：入参统一为单一 Args 结构体，便于向后兼容地加可选字段）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFulltextArgs {
    pub query: String,
    #[serde(default)]
    pub options: SearchOptions,
    #[serde(default)]
    pub page: PageArg,
}

/// search_quick_open 入参（PRD：{ query, limit }）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpenArgs {
    pub query: String,
    pub limit: Option<usize>,
}

/// 文件内查找选项（FR-EDITOR-09：区分大小写、全字匹配；FR-SEARCH-31 明示「复用 FR-EDITOR-09」）。
///
/// **没有 regex 字段**：FR-EDITOR-09 的「正则」需要 regex crate，而本批的约束是零新依赖
/// （技术方案 §3.5.2 矩阵把 regex 列为 M3 引入项，M3 实际未引入）。与其放一个会静默降级的开关，
/// 不如先不提供 —— 见 PR 报告的口径缺口（补齐方式由项目所有者裁定）。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindOptionsArg {
    pub case_sensitive: Option<bool>,
    pub whole_word: Option<bool>,
}

/// search_in_file 入参（PRD：{ rel_path, query, options }）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFileArgs {
    pub rel_path: String,
    pub query: String,
    #[serde(default)]
    pub options: FindOptionsArg,
}

/// replace_in_file 的一处目标：来自 search_in_file 的 InFileMatch（字节区间 + 期望原文）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceMatchArg {
    pub start_byte: usize,
    pub end_byte: usize,
    /// 查找时那一处的原文；替换前拿它逐条比对（陈旧检测，R-07）
    pub text: String,
}

/// replace_in_file 入参（PRD：{ rel_path, matches, replacement }）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceArgs {
    pub rel_path: String,
    pub matches: Vec<ReplaceMatchArg>,
    pub replacement: String,
}

/// 数值夹紧：缺省 → 缺省值；越界 → 夹到边界（而不是报错或原样透传）。
pub(crate) fn clamp_i64(value: Option<i64>, default: i64, min: i64, max: i64) -> i64 {
    value.unwrap_or(default).clamp(min, max)
}

/// 同 clamp_i64，用于 usize 选项。
pub(crate) fn clamp_usize(value: Option<usize>, default: usize, min: usize, max: usize) -> usize {
    value.unwrap_or(default).clamp(min, max)
}

/// 全局库句柄（搜索历史存于全局库，FR-SEARCH-10「存于全局库」）。
pub(crate) fn global_pool(state: &AppState) -> Result<Arc<DbPool>, KpError> {
    state
        .global_db()
        .ok_or(KpError(AppError::db("访问搜索历史")))
}

/// 读取搜索历史（FR-SEARCH-10）：最近 20 条，最新在前。
pub(crate) fn search_history_get_impl(state: &AppState) -> Result<Vec<String>, KpError> {
    let pool = global_pool(state)?;
    crate::storage::search_history::load(&pool).map_err(KpError)
}

/// 清空搜索历史（⚠️ 写操作）。
pub(crate) fn search_history_clear_impl(state: &AppState) -> Result<(), KpError> {
    let pool = global_pool(state)?;
    crate::storage::search_history::clear(&pool).map_err(KpError)?;
    // CMD-06 / ERR-04：写操作留痕。**不记任何查询内容**（SEC-09 / R-12）。
    tracing::info!(action = "search_history_clear", "搜索历史已清空");
    Ok(())
}

/// 记入一条搜索历史（**尽力而为**：历史写失败绝不能让搜索失败）。
///
/// 只在 options.recordHistory = true 时调用；日志里**只有条数**（SEC-09 / R-12）。
pub(crate) fn record_history_best_effort(state: &AppState, query: &str) {
    let Ok(pool) = global_pool(state) else {
        tracing::debug!("全局库不可用，本次不记录搜索历史");
        return;
    };
    match crate::storage::search_history::record(&pool, query) {
        Ok(list) => tracing::debug!(entries = list.len(), "已记入一条搜索历史"),
        Err(err) => tracing::warn!(kind = err.kind(), "搜索历史写入失败（不影响本次搜索）"),
    }
}

// ── 6 个命令（薄壳：只做参数搬运与调用，CMD-01）──────────────────────────────

/// 全文搜索（PRD §5.3.4；FR-SEARCH-01~09）。
#[tauri::command]
pub async fn search_fulltext(
    state: State<'_, AppState>,
    args: SearchFulltextArgs,
) -> Result<SearchResultPage, KpError> {
    search_fulltext_impl(&state, args)
}

/// 文件名模糊匹配（PRD §5.3.4；FR-SEARCH-40/41）。
#[tauri::command]
pub async fn search_quick_open(
    state: State<'_, AppState>,
    args: QuickOpenArgs,
) -> Result<Vec<QuickOpenItem>, KpError> {
    search_quick_open_impl(&state, args)
}

/// 文件内查找（PRD §5.3.4；FR-SEARCH-30）。
#[tauri::command]
pub async fn search_in_file(
    state: State<'_, AppState>,
    args: InFileArgs,
) -> Result<Vec<InFileMatch>, KpError> {
    search_in_file_impl(&state, args)
}

/// 文件内替换（PRD §5.3.4 标注 ⚠️；FR-SEARCH-31）。
#[tauri::command]
pub async fn replace_in_file(
    state: State<'_, AppState>,
    args: ReplaceArgs,
) -> Result<ReplaceResult, KpError> {
    replace_in_file_impl(&state, args)
}

/// 搜索历史（PRD §5.3.4；FR-SEARCH-10）。
#[tauri::command]
pub async fn search_history_get(state: State<'_, AppState>) -> Result<Vec<String>, KpError> {
    search_history_get_impl(&state)
}

/// 清空搜索历史（PRD §5.3.4 标注 ⚠️；FR-SEARCH-10）。
#[tauri::command]
pub async fn search_history_clear(state: State<'_, AppState>) -> Result<(), KpError> {
    search_history_clear_impl(&state)
}
