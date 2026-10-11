//! 链接查询域命令的测试（判据都打在命令层，见 link.rs 的 DEBT-14 说明）。
//!
//! 覆盖 6 个只读命令：反链（含嵌入过滤与片段）、出链、悬空、歧义（含 linkIds）、
//! 孤立、标题；以及"未打开 Vault"的错误路径。

use std::sync::Arc;

use crate::commands::link::{
    link_ambiguous_list_impl, link_backlinks_impl, link_dangling_list_impl, link_headings_impl,
    link_orphan_list_impl, link_outgoing_impl,
};
use crate::state::AppState;

/// 建临时 Vault + 已建索引的 AppState。
fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).expect("建目录");
        }
        std::fs::write(&path, content).expect("写文件");
    }
    let pool = crate::storage::index::open(&root).expect("建索引库");
    crate::index_engine::full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let state = AppState::new();
    state.set_root(Some(root));
    state.set_index_db(Some(Arc::new(pool)));
    (dir, state)
}

#[test]
fn backlinks_group_by_source_and_split_embeds() {
    let (_d, state) = fixture(&[("A.md", "# 甲\n"), ("B.md", "见 [[A]]\n\n也见 ![[A]]\n")]);
    let all = link_backlinks_impl(&state, "A.md", true).expect("反链");
    assert_eq!(all.len(), 1, "同一个来源归一组");
    assert_eq!(all[0].src_rel_path, "B.md");
    assert_eq!(all[0].link_count, 1, "普通链接");
    assert_eq!(all[0].embed_count, 1, "嵌入单独计数（FR-LINK-15）");
    assert_eq!(all[0].items.len(), 2);
    assert!(all[0].items.iter().any(|i| i.link_kind == "embed"));

    let no_embed = link_backlinks_impl(&state, "A.md", false).expect("反链");
    assert_eq!(
        no_embed[0].embed_count, 0,
        "include_embeds=false 时过滤嵌入"
    );
    assert_eq!(no_embed[0].items.len(), 1);
    assert_eq!(no_embed[0].link_count, 1, "计数与条数必须一致");
}

#[test]
fn backlinks_carry_snippet_and_line() {
    let (_d, state) = fixture(&[("A.md", "# 甲\n"), ("B.md", "第一行\n第二行 [[A]] 尾巴\n")]);
    let groups = link_backlinks_impl(&state, "A.md", true).expect("反链");
    let item = &groups[0].items[0];
    assert_eq!(item.line, 2, "行号 1-based");
    let snippet = item.snippet.as_ref().expect("应带上下文片段（FR-LINK-11）");
    assert_eq!(snippet.highlight_line, 2);
    assert!(
        snippet.lines.iter().any(|l| l.text.contains("[[A]]")),
        "片段里应能看到链接本身"
    );
}

#[test]
fn outgoing_lists_targets_and_status() {
    let (_d, state) = fixture(&[("A.md", "见 [[B]] 与 [[missing]]\n"), ("B.md", "# 乙\n")]);
    let out = link_outgoing_impl(&state, "A.md").expect("出链");
    assert_eq!(out.len(), 2);
    let b = out.iter().find(|o| o.target_ref == "B").expect("B 出链");
    assert_eq!(b.status, "resolved");
    let m = out
        .iter()
        .find(|o| o.target_ref == "missing")
        .expect("missing 出链");
    assert_eq!(m.status, "dangling", "指向不存在的笔记应为 dangling");
}

#[test]
fn dangling_groups_count_refs_and_sources() {
    let (_d, state) = fixture(&[("one.md", "[[ghost]]\n"), ("two.md", "也见 [[ghost]]\n")]);
    let page = link_dangling_list_impl(&state).expect("悬空清单");
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    let g = &page.items[0];
    assert_eq!(g.target_ref, "ghost");
    assert_eq!(g.ref_count, 2, "两处引用");
    assert_eq!(g.source_count, 2, "来自两篇");
    assert_eq!(g.sample_sources.len(), 2);
}

#[test]
fn ambiguous_lists_candidates_and_link_ids() {
    let (_d, state) = fixture(&[
        ("f1/note.md", "# 一\n"),
        ("f2/note.md", "# 二\n"),
        ("src.md", "见 [[note]]\n"),
    ]);
    let page = link_ambiguous_list_impl(&state).expect("歧义清单");
    assert_eq!(page.total, 1);
    let item = &page.items[0];
    assert_eq!(item.target_ref, "note");
    assert_eq!(
        item.candidates,
        vec!["f1/note.md", "f2/note.md"],
        "候选按路径排序"
    );
    assert_eq!(item.ref_count, 1);
    assert_eq!(item.link_ids.len(), 1, "linkIds 要贯通到命令层（#93）");
}

#[test]
fn orphans_list_notes_without_links() {
    let (_d, state) = fixture(&[("solo.md", "没人引用我\n"), ("A.md", "# 甲\n")]);
    let page = link_orphan_list_impl(&state).expect("孤立清单");
    assert!(
        page.items.contains(&"solo.md".to_string()),
        "无入链的笔记应被列出"
    );
}

#[test]
fn headings_expose_level_text_anchor_line() {
    let (_d, state) = fixture(&[("A.md", "# 一级\n正文\n## 二级 标题\n")]);
    let heads = link_headings_impl(&state, "A.md").expect("标题");
    assert_eq!(heads.len(), 2);
    assert_eq!(heads[0].level, 1);
    assert_eq!(heads[0].text, "一级");
    assert_eq!(heads[0].line, 1);
    assert_eq!(heads[1].level, 2);
    assert!(!heads[1].anchor.is_empty(), "锚点供 FR-EDITOR-23 补全用");
}

#[test]
fn commands_error_when_vault_not_open() {
    let state = AppState::new();
    for err in [
        link_backlinks_impl(&state, "A.md", true).expect_err("反链应报错"),
        link_dangling_list_impl(&state).expect_err("悬空应报错"),
        link_ambiguous_list_impl(&state).expect_err("歧义应报错"),
        link_orphan_list_impl(&state).expect_err("孤立应报错"),
        link_headings_impl(&state, "A.md").expect_err("标题应报错"),
        link_outgoing_impl(&state, "A.md").expect_err("出链应报错"),
    ] {
        assert_eq!(err.0.code(), "E_VAULT_NOT_OPEN");
    }
}
