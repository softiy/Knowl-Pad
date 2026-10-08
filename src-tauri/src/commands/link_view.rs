//! 链接查询域的 **IPC 形状**（PRD §5.3.5 的返回值）。
//!
//! 单独成文件是刻意的：命令实现与形状各占一个文件，两者都远低于 CODE-11 的 200 行警告线，
//! 后续加命令时也不会再触发"文件超长"。

use serde::Serialize;

use kp_domain::path_guard::PathGuard;
use kp_domain::snippet::{extract_snippet, DEFAULT_CONTEXT_LINES};

/// 片段里的一行。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetLineView {
    pub line: u32,
    pub text: String,
    pub is_link_line: bool,
}

/// 反链的上下文片段（FR-LINK-11/12）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetView {
    pub lines: Vec<SnippetLineView>,
    pub highlight_line: u32,
    pub highlight_start: usize,
    pub highlight_end: usize,
}

/// 一条反链（FR-LINK-11）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacklinkItem {
    pub line: u32,
    pub col: u32,
    pub link_kind: String,
    pub alias: Option<String>,
    pub anchor: Option<String>,
    pub snippet: Option<SnippetView>,
}

/// 按来源分组的一组反链（FR-LINK-14/15：按来源分组，嵌入与普通分开计数）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacklinkGroup {
    pub src_rel_path: String,
    pub src_name: String,
    pub link_count: usize,
    pub embed_count: usize,
    pub items: Vec<BacklinkItem>,
}

/// 出链（含状态）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingLink {
    pub target_ref: String,
    pub status: String,
    pub anchor: Option<String>,
    pub alias: Option<String>,
    pub link_kind: String,
    pub line: u32,
    pub col: u32,
    pub dst_rel_path: Option<String>,
}

/// 标题（`link_headings`）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadingItem {
    pub level: u32,
    pub text: String,
    pub anchor: String,
    pub line: u32,
}

/// 悬空链接一项（FR-LINK-20）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DanglingItem {
    pub target_ref: String,
    pub ref_count: u32,
    pub source_count: u32,
    pub sample_sources: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DanglingPage {
    pub items: Vec<DanglingItem>,
    pub total: usize,
}

/// 歧义链接一项（FR-LINK-22 / AC-LINK-04）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousItem {
    pub target_ref: String,
    pub candidates: Vec<String>,
    pub ref_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousPage {
    pub items: Vec<AmbiguousItem>,
    pub total: usize,
}

/// 孤立笔记清单（FR-LINK-21）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanPage {
    pub items: Vec<String>,
    pub total: usize,
}

/// 读笔记正文（片段用）。路径经 PathGuard 七步校验（SEC-02）；读不到就返回 None（索引与磁盘短暂不一致时降级）。
pub fn read_note(root: &std::path::Path, rel_path: &str) -> Option<String> {
    let guard = PathGuard::new(root).ok()?;
    let path = guard.resolve(rel_path).ok()?;
    std::fs::read_to_string(path).ok()
}

/// 取链接所在行的上下文片段（默认前后各 2 行，FR-LINK-11）。
pub fn snippet_of(content: &str, line: u32, col: u32, link_len: usize) -> Option<SnippetView> {
    extract_snippet(
        content,
        line,
        col,
        link_len,
        DEFAULT_CONTEXT_LINES,
        DEFAULT_CONTEXT_LINES,
    )
    .map(|s| SnippetView {
        lines: s
            .lines
            .into_iter()
            .map(|l| SnippetLineView {
                line: l.line,
                text: l.text,
                is_link_line: l.is_link_line,
            })
            .collect(),
        highlight_line: s.highlight_line,
        highlight_start: s.highlight_start,
        highlight_end: s.highlight_end,
    })
}
