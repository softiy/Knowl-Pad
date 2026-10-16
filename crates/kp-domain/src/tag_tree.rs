//! 标签树构建（FR-TAG-01/02、AC-TAG-01/03、FR-TAG-13）。
//!
//! 放在 domain 的理由：这是**纯逻辑** —— 输入是 `tag` 表的行，输出是树；不碰数据库、不碰 IPC。
//! 放在这里可以直接对着 **AC-TAG-01 的原文形状**单测（「项目 → alpha → {前端, 后端}、项目 → beta、随笔」），
//! 不必造八张表的夹具。
//!
//! **计数口径（2026-10-16 实测后定稿，与 W0 初稿不同 —— 见 PRD §5.3.6.1 的说明）**：
//! `tag.ref_count` 是 **file_tag 行数**，不是"笔记篇数"。两个后果都由实测确认：
//! ① AC-TAG-04 / FR-TAG-12 原文就把它定义成 `COUNT(*) FROM file_tag`（行数）——**这是权威口径**；
//! ② 因为 MD-TAG-03 要求"层级标签同时登记自身与全部祖先"，**祖先的 ref_count 天然已含全部子孙**
//!    （`#项目/alpha/前端` 会给 `项目/alpha`、`项目` 各写一行）⇒ AC-TAG-01 的"各级计数为其自身及全部子孙的引用总和"
//!    **已经由 ref_count 本身满足**，不需要（也不允许）再逐级相加 —— 相加会**重复计数**。
//!    因此本模块**不提供** `total_count`（W0 初稿里那个字段是基于错误假设写的，已在 PR-A 删掉并改契约）。
//!
//! `is_leaf` 直接取行里的值（AC-TAG-01「仅最末级为真」由索引侧保证）。

use std::collections::BTreeMap;

/// 「未分类」节点的 `norm` 约定（空串；见 PRD §5.3.6.1 的决策说明）。
pub const UNCLASSIFIED_NORM: &str = "";

/// `tag` 表的一行（只取建树需要的列，避免 domain 依赖存储层类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRow {
    pub norm: String,
    pub display: String,
    pub depth: i64,
    pub is_leaf: bool,
    pub ref_count: i64,
}

/// 树节点（IPC 形状见 `src-tauri/src/commands/tag_view.rs`；契约见 PRD §5.3.6.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagNode {
    pub norm: String,
    pub display: String,
    pub depth: i64,
    pub is_leaf: bool,
    /// 该标签的引用计数（= `tag.ref_count`，**行数口径**：见模块头的"计数口径"一节）。
    pub ref_count: i64,
    /// 仅「未分类」条目为 true（FR-TAG-13）。
    pub unclassified: bool,
    /// 子节点，按 `norm` 升序。
    pub children: Vec<TagNode>,
}

/// 找 `norm` 的父标签：取**最长的、真实存在的**前缀（`a/b/c` 先试 `a/b`，再试 `a`）。
///
/// 找不到就返回 `None`（当作根）—— 例如 `a/b` 存在但 `a` 不在表里（理论上 MD-TAG-03 保证不会，
/// 但真库可能因手工改库/旧版本数据而出现）⇒ **挂到根而不是丢弃**，避免面板"少一个标签"这种静默丢数据。
fn parent_of(norm: &str, exists: &BTreeMap<&str, &TagRow>) -> Option<String> {
    let mut cut = norm.rfind('/')?;
    loop {
        let candidate = &norm[..cut];
        if exists.contains_key(candidate) {
            return Some(candidate.to_string());
        }
        match candidate.rfind('/') {
            Some(i) => cut = i,
            None => return None,
        }
    }
}

/// 把一个节点及其子孙物化成 `TagNode`，并回填 `total_count`。
fn materialize(
    norm: &str,
    rows: &BTreeMap<&str, &TagRow>,
    kids: &BTreeMap<String, Vec<String>>,
) -> TagNode {
    let row = rows[norm];
    let children: Vec<TagNode> = match kids.get(norm) {
        Some(list) => list
            .iter()
            .map(|child| materialize(child, rows, kids))
            .collect(),
        None => Vec::new(),
    };
    TagNode {
        norm: row.norm.clone(),
        display: row.display.clone(),
        depth: row.depth,
        is_leaf: row.is_leaf,
        ref_count: row.ref_count,
        unclassified: false,
        children,
    }
}

/// 把 `tag` 表的行建成**树**（根节点列表，同级按 `norm` 升序）。
///
/// 层级关系由 `norm` 的 `/` 前缀推导（`a/b` 是 `a` 的孩子）。
pub fn build_tree(rows: &[TagRow]) -> Vec<TagNode> {
    let by_norm: BTreeMap<&str, &TagRow> = rows.iter().map(|r| (r.norm.as_str(), r)).collect();
    let mut kids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut roots: Vec<String> = Vec::new();
    for row in rows {
        match parent_of(&row.norm, &by_norm) {
            Some(parent) => kids.entry(parent).or_default().push(row.norm.clone()),
            None => roots.push(row.norm.clone()),
        }
    }
    roots.sort_unstable();
    roots
        .iter()
        .map(|norm| materialize(norm, &by_norm, &kids))
        .collect()
}

/// 「未分类」节点（FR-TAG-13）：无标签的笔记数。
///
/// 为什么是**合成节点**而不是改 `tag_tree` 的返回类型：PRD §5.3.6 已把返回钉死为 `TagNode[]`，
/// 改成对象属于**改契约形状**；用一个 `unclassified: true` 的节点既不动形状，又让面板"遍历同一个数组"即可渲染。
/// 调用方负责把它**排在全部标签之后**。
pub fn unclassified_node(count: i64) -> TagNode {
    TagNode {
        norm: UNCLASSIFIED_NORM.to_string(),
        display: "未分类".to_string(),
        depth: 0,
        is_leaf: true,
        ref_count: count,
        unclassified: true,
        children: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(norm: &str, display: &str, depth: i64, is_leaf: bool, ref_count: i64) -> TagRow {
        TagRow {
            norm: norm.to_string(),
            display: display.to_string(),
            depth,
            is_leaf,
            ref_count,
        }
    }

    /// **AC-TAG-01 原文即用例**：`#项目/alpha/前端`、`#项目/alpha/后端`、`#项目/beta`、`#随笔`
    /// ⇒ 树形「项目 → alpha → {前端, 后端}、项目 → beta、随笔」。
    #[test]
    fn ac_tag_01_tree_shape_matches_the_acceptance_text() {
        let rows = vec![
            row("项目/alpha/前端", "项目/alpha/前端", 3, true, 2),
            row("项目/alpha/后端", "项目/alpha/后端", 3, true, 1),
            row("项目/alpha", "项目/alpha", 2, false, 0),
            row("项目/beta", "项目/beta", 2, true, 3),
            row("项目", "项目", 1, false, 0),
            row("随笔", "随笔", 1, true, 4),
        ];
        let tree = build_tree(&rows);
        // 顺序口径 = **按 norm 升序**（Unicode 码点序：随 U+968F < 项 U+9879）——
        // AC-TAG-01 的原文枚举的是"树里有谁"，不是顺序；顺序由本实现的稳定规则决定，这里把它钉住。
        let roots: Vec<&str> = tree.iter().map(|n| n.norm.as_str()).collect();
        assert_eq!(roots, vec!["随笔", "项目"], "根按 norm 升序");

        let project = &tree[1];
        let kids: Vec<&str> = project.children.iter().map(|n| n.norm.as_str()).collect();
        assert_eq!(kids, vec!["项目/alpha", "项目/beta"], "项目 → alpha、beta");
        let alpha_kids: Vec<&str> = project.children[0]
            .children
            .iter()
            .map(|n| n.norm.as_str())
            .collect();
        assert_eq!(
            alpha_kids,
            vec!["项目/alpha/前端", "项目/alpha/后端"],
            "alpha → 前端、后端"
        );
    }

    /// 计数口径（2026-10-16 实测定稿）：**build_tree 不做任何求和** ——
    /// tag.ref_count 由索引侧的祖先登记天然含子孙（见模块头"计数口径"），再求和就是**重复计数**。
    /// 本用例就是这条口径的绊线：谁把求和加回来，数字会立刻变成 12 而不是 6。
    #[test]
    fn ref_count_is_passed_through_and_never_summed() {
        let rows = vec![
            row("项目/alpha/前端", "项目/alpha/前端", 3, true, 2),
            row("项目/alpha/后端", "项目/alpha/后端", 3, true, 1),
            row("项目/alpha", "项目/alpha", 2, false, 3),
            row("项目/beta", "项目/beta", 2, true, 3),
            row("项目", "项目", 1, false, 6),
        ];
        let tree = build_tree(&rows);
        let project = &tree[0];
        assert_eq!(
            project.ref_count, 6,
            "原样透出（索引写入时已是 3+3）；叠加子孙会变成 12"
        );
        assert_eq!(
            project.children[0].ref_count, 3,
            "alpha 的 3 = 前端 2 + 后端 1"
        );
        assert!(!project.is_leaf, "有子孙 ⇒ 非末级");
        assert!(project.children[0].children[0].is_leaf, "末级 is_leaf 为真");
    }

    /// 数据异常兜底：父标签不在表里时**挂到根**（不丢数据），而不是整枝消失。
    #[test]
    fn orphan_branch_falls_back_to_root_instead_of_disappearing() {
        let rows = vec![row("a/b", "a/b", 2, true, 1)];
        let tree = build_tree(&rows);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].norm, "a/b", "找不到父节点 ⇒ 当根，不丢弃");
        assert_eq!(tree[0].ref_count, 1);
    }

    /// FR-TAG-13：未分类节点是**合成**的（norm 空串、unclassified 为真、计数即笔记数）。
    #[test]
    fn unclassified_node_carries_the_count_and_a_flag() {
        let node = unclassified_node(7);
        assert_eq!(node.norm, UNCLASSIFIED_NORM);
        assert!(node.unclassified);
        assert_eq!(
            node.ref_count, 7,
            "未分类条目的计数是**笔记篇数**（它没有 file_tag 行）"
        );
        assert!(node.children.is_empty());
        assert!(node.is_leaf);
    }

    /// 大小写：树只认 norm（小写规范键），显示名走 display（AC-TAG-03 的两个字段各司其职）。
    #[test]
    fn tree_keys_on_norm_and_displays_display() {
        let rows = vec![row("work", "Work", 1, true, 3)];
        let tree = build_tree(&rows);
        assert_eq!(tree[0].norm, "work");
        assert_eq!(tree[0].display, "Work");
    }
}
