//! 链接查询层的**行结构**（从 `link_query.rs` 拆出，避免单文件越过 CODE-11 的 200 行警告线）。
//!
//! 纯数据结构，不含逻辑；SQL 与查询函数仍全在 `link_query.rs`。

use serde::Serialize;

/// 一条反链（FR-LINK-10/11/15）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // 命令层接入前暂无调用方（写明了移除点）
pub struct BacklinkRow {
    pub src_rel_path: String,
    pub src_name: String,
    pub link_kind: String,
    pub target_ref: String,
    pub alias: Option<String>,
    pub anchor: Option<String>,
    pub line: u32,
    pub col: u32,
}

/// 一条出链（含悬空/歧义状态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // 命令层接入前暂无调用方（写明了移除点）
pub struct OutgoingRow {
    pub target_ref: String,
    pub status: String,
    pub anchor: Option<String>,
    pub alias: Option<String>,
    pub link_kind: String,
    pub line: u32,
    pub col: u32,
    pub dst_rel_path: Option<String>,
}

/// 悬空链接按目标名分组（FR-LINK-20）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // 命令层接入前暂无调用方（写明了移除点）
pub struct DanglingGroup {
    pub target_ref: String,

    pub ref_count: u32,

    pub source_count: u32,

    pub sample_sources: Vec<String>,
}

/// 歧义链接（FR-LINK-22 / AC-LINK-04）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // 命令层接入前暂无调用方（写明了移除点）
pub struct AmbiguousRow {
    pub target_ref: String,

    pub candidates: Vec<String>,

    pub ref_count: u32,
}

/// 标题（link_headings 用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // 命令层接入前暂无调用方（写明了移除点）
pub struct HeadingRow {
    pub level: u32,
    pub text: String,
    pub anchor: String,
    pub line: u32,
}
