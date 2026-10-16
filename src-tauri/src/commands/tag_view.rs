//! 标签域的 IPC 形状（PRD **§5.3.6.1** 的 `TagNode` / `TaggedNote`；契约表即本节）。
//!
//! 字段与 PRD §5.3.6.1 逐条对应，IPC 传输键为 **camelCase**（技术方案 §8.4），Rust 内部字段 snake_case。
//! 树节点的构建逻辑在 `kp_domain::tag_tree`（纯逻辑 + 单测），本文件只负责"翻译成线格式"。

use serde::Serialize;

/// 标签树节点（FR-TAG-01/02；AC-TAG-01 的计数与 isLeaf 语义见字段注释）。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TagNode {
    /// 小写规范键（`a/b/c`；AC-TAG-03）。
    pub norm: String,
    /// 显示名（**首次出现**的原文形式；取自 `tag.display`）。
    pub display: String,
    /// 层级深度（`a/b/c` = 3）。
    pub depth: i64,
    /// 是否末级（`tag.is_leaf`；AC-TAG-01「仅最末级为真」）。
    pub is_leaf: bool,
    /// 引用计数（`tag.ref_count`，**file_tag 行数**口径；祖先天然含子孙 —— 见 PRD §5.3.6.1）。
    pub ref_count: i64,
    /// 仅「未分类」条目为 true（FR-TAG-13）。
    pub unclassified: bool,
    pub children: Vec<TagNode>,
}

impl From<kp_domain::tag_tree::TagNode> for TagNode {
    fn from(n: kp_domain::tag_tree::TagNode) -> Self {
        Self {
            norm: n.norm,
            display: n.display,
            depth: n.depth,
            is_leaf: n.is_leaf,
            ref_count: n.ref_count,
            unclassified: n.unclassified,
            children: n.children.into_iter().map(Into::into).collect(),
        }
    }
}

/// 某标签下的一篇笔记（FR-TAG-03）。
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaggedNote {
    pub rel_path: String,
    pub name: String,
    pub mtime_ms: i64,
}

impl From<crate::storage::tag_query::TaggedNoteRow> for TaggedNote {
    fn from(r: crate::storage::tag_query::TaggedNoteRow) -> Self {
        Self {
            rel_path: r.rel_path,
            name: r.name,
            mtime_ms: r.mtime_ms,
        }
    }
}
