//! 搜索域的 IPC 形状（PRD §5.3.4 的 SearchResultPage / QuickOpenItem / InFileMatch / ReplaceResult）。
//!
//! **口径缺口（已在 PR 报告登记，未擅自改真源）**：PRD §5.3.4（第 1648–1653 行）只给出返回类型的
//! **名字**，字段从未定义 —— 与勘误 D-11（文件域）/ D-18（索引域）同类。M3 计划 WP1 第 3 条
//! 写过「形状补进 PRD §5.3.4」，但真源里至今没有这一节。本文件按 FR-SEARCH-01~09 与
//! PRD §4.11.3 的设置项**逐字段注明来源**，字段名保持 camelCase（技术方案 §8.4）。

use serde::Serialize;

use kp_domain::find_in_file::LocatedMatch;
use kp_domain::search_hits::Hit;
use kp_domain::snippet::{extract_snippet, Snippet};

/// 片段中的一行。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnippetLineView {
    pub line: u32,
    pub text: String,
    /// 命中所在行（与反链片段的 isLinkLine 同义，按搜索语境改名）
    pub is_hit_line: bool,
}

/// 命中片段（FR-SEARCH-04：含前后上下文，命中词高亮）。
///
/// highlightStart / highlightEnd 相对 highlightLine 的文本，**0 基、左闭右开、字符**偏移 ——
/// 与反链片段（kp_domain::snippet）完全同一口径，前端可复用同一套高亮渲染。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnippet {
    pub lines: Vec<SnippetLineView>,
    pub highlight_line: u32,
    pub highlight_start: usize,
    pub highlight_end: usize,
}

impl From<Snippet> for SearchSnippet {
    fn from(s: Snippet) -> Self {
        Self {
            lines: s
                .lines
                .into_iter()
                .map(|l| SnippetLineView {
                    line: l.line,
                    text: l.text,
                    is_hit_line: l.is_link_line,
                })
                .collect(),
            highlight_line: s.highlight_line,
            highlight_start: s.highlight_start,
            highlight_end: s.highlight_end,
        }
    }
}

/// 取命中处的上下文片段（前后各 context_lines 行，PRD §4.11.3 的「片段上下文行数」，默认 2）。
///
/// 行号越界（索引与磁盘短暂不一致、空文件）返回 None，由调用方降级 —— 不 panic、不整单失败。
pub fn snippet_at(content: &str, hit: &Hit, context_lines: usize) -> Option<SearchSnippet> {
    extract_snippet(
        content,
        hit.line,
        hit.col,
        hit.len_chars,
        context_lines,
        context_lines,
    )
    .map(SearchSnippet::from)
}

/// 全文搜索的一条结果（FR-SEARCH-04/05：同一笔记多处命中**合并为一条**）。
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub rel_path: String,
    pub name: String,
    /// 文件修改时间（FR-SEARCH-04 要求每条结果显示修改时间）
    pub mtime_ms: i64,
    /// bm25 相关度（负值，越小越相关；排序口径见 storage::search_query::SortOrder）
    pub score: f64,
    /// 命中**次数**（合并相邻/重叠后的处数，FR-SEARCH-05）
    pub hit_count: usize,
    /// 上下文片段：**首个片段即 snippets[0]**（不重复传输同一份数据，IPC-05 的载荷纪律）；
    /// FR-SEARCH-05 的「可展开查看全部片段」= 展开这个数组（上限由 options.maxSnippets 控制）
    pub snippets: Vec<SearchSnippet>,
}

/// 一页结果（PRD §5.3.4 的 SearchResultPage）。
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultPage {
    pub items: Vec<SearchResultItem>,
    /// 命中**总数**（不是本页条数；IPC-05 要求大批量必须分页）
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
    /// 是否还有下一页（前端据此决定是否请求更多）
    pub has_more: bool,
}

/// 快速打开候选（PRD §5.3.4 的 QuickOpenItem）。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpenItem {
    pub rel_path: String,
    pub name: String,
    /// 模糊匹配得分（口径见 kp_domain::quick_open）；越大越靠前
    pub score: i32,
}

/// 文件内的一处命中（PRD §5.3.4 的 InFileMatch；FR-SEARCH-30）。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InFileMatch {
    pub line: u32,
    /// 1 基字符列（中文按 1 个字符算）
    pub col: u32,
    pub len_chars: usize,
    /// 原文中的字节区间：replace_in_file 按它精确落刀，不靠行列二次解析
    pub start_byte: usize,
    pub end_byte: usize,
    /// 命中的原文（替换时作为**期望原文**回传，用于陈旧检测）
    pub text: String,
    /// 命中所在整行文本（展示「第 X / 共 Y 处」时一并给出上下文）
    pub line_text: String,
    /// 高亮区间：相对 lineText，0 基、左闭右开、字符
    pub highlight_start: usize,
    pub highlight_end: usize,
}

impl InFileMatch {
    /// 由域层命中映射为 IPC 形状（text 必须取自**当前**正文，它同时是替换时的陈旧检测基准）。
    pub fn of(content: &str, located: &LocatedMatch) -> Self {
        let site = &located.site;
        let text = content
            .get(site.start_byte..site.end_byte)
            .unwrap_or_default()
            .to_string();
        let highlight_start = site.col.saturating_sub(1) as usize;
        Self {
            line: site.line,
            col: site.col,
            len_chars: site.len_chars,
            start_byte: site.start_byte,
            end_byte: site.end_byte,
            text,
            line_text: located.line_text.clone(),
            highlight_start,
            highlight_end: highlight_start + site.len_chars,
        }
    }
}

/// 文件内替换结果（PRD §5.3.4 的 ReplaceResult）。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceResult {
    pub rel_path: String,
    /// 实际替换的处数（= 通过陈旧检测的目标数；一处不一致即整体中止，见 replace_in_file 的说明）
    pub replaced: usize,
    pub new_mtime_ms: i64,
}
