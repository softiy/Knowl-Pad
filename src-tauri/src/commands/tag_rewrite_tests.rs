//! 标签重命名的命令面测试（判据打在命令体上；真实 Vault + 真实索引库）。
//!
//! 对着验收原文：
//! - **AC-TAG-02**：80 篇里全部出现被改写、frontmatter 的 tags 同步更新、改写前**备份全部文件**、
//!   树更新为新名、旧标签无其他子孙时从标签表消失；**任一文件失败 ⇒ 全部回滚、标签表保持原状**；
//! - **FR-TAG-07**：层级子标签的**前缀更新**；
//! - **FR-TAG-08**：非法嵌套/循环在预览阶段就被拒绝（磁盘零改动）。

use std::sync::Arc;

use crate::commands::tag::{tag_notes_impl, tag_tree_impl, TagNotesArgs};
use crate::commands::tag_rewrite::{tag_rename_apply_impl, tag_rename_preview_impl};
use crate::state::AppState;

/// 建临时 Vault（真实文件）+ 真实全量索引（与 tag_tests / search_tests 的夹具同源）。
fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, AppState) {
    let vault = tempfile::tempdir().expect("Vault 临时目录");
    let root = vault.path().to_path_buf();
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).expect("建目录");
        }
        std::fs::write(&path, content).expect("写文件");
    }
    let pool = crate::storage::index::open(&root).expect("建索引库");
    crate::index_engine::full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let config = tempfile::tempdir().expect("配置目录");
    let global = crate::storage::global::open(config.path()).expect("建全局库");
    let state = AppState::new();
    state.set_root(Some(root));
    state.set_index_db(Some(Arc::new(pool)));
    state.set_global_db(Arc::new(global));
    (vault, state)
}

fn root_of(state: &AppState) -> std::path::PathBuf {
    state.current_root().expect("Vault 根")
}

fn read(root: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("读 {rel} 失败：{e}"))
}

/// 重新全量索引（改写后按 AC-TAG-02 要看"标签面板树更新"）。
fn reindex(state: &AppState) {
    let root = root_of(state);
    let pool = state.index_db().expect("索引库");
    crate::index_engine::full_index(&pool, &root, |_, _| {}).expect("重索引");
}

/// 树里所有 norm（含子孙），按字典序 —— 便于断言"旧标签消失、新标签出现"。
fn all_norms(state: &AppState) -> Vec<String> {
    fn walk(nodes: &[crate::commands::tag_view::TagNode], out: &mut Vec<String>) {
        for n in nodes {
            if !n.unclassified {
                out.push(n.norm.clone());
            }
            walk(&n.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&tag_tree_impl(state).expect("标签树"), &mut out);
    out.sort();
    out
}

/// **AC-TAG-02 主路径**：正文与 frontmatter 的全部出现被改写、备份存在、树更新、旧标签消失。
#[test]
fn ac_tag_02_renames_everywhere_backs_up_and_updates_the_tree() {
    let (_v, state) = fixture(&[
        ("a.md", "---\ntags: [old/name]\n---\n\n正文 #old/name\n"),
        ("b.md", "#old/name 与 #old/name\n"),
        ("c.md", "---\ntags: [old/name, 其他]\n---\n正文无标签\n"),
        ("d.md", "#old/name/深 子标签\n"),
        ("keep.md", "#无关 标签\n"),
    ]);
    let preview =
        tag_rename_preview_impl(&state, "old/name".into(), "new/name".into()).expect("预览");
    assert_eq!(
        preview.file_count,
        4,
        "4 篇命中（a/b/c/d）；实得：{:?}",
        preview
            .edits
            .iter()
            .map(|e| &e.rel_path)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        preview.span_count, 6,
        "a frontmatter1 + a 正文1 + b 两处 + c frontmatter1 + d 子标签1"
    );
    assert!(preview.skipped.is_empty());

    let result = tag_rename_apply_impl(
        &state,
        "old/name".into(),
        "new/name".into(),
        preview.preview_id.clone(),
    )
    .expect("落地");
    assert_eq!(result.file_count, 4);
    assert_eq!(result.span_count, 6);

    let root = root_of(&state);
    assert!(
        read(&root, "a.md").contains("tags: [new/name]"),
        "frontmatter 同步"
    );
    assert!(read(&root, "a.md").contains("#new/name"));
    assert_eq!(read(&root, "b.md").matches("#new/name").count(), 2);
    assert!(
        read(&root, "c.md").contains("tags: [new/name, 其他]"),
        "无关标签不动"
    );
    assert!(
        read(&root, "d.md").contains("#new/name/深"),
        "FR-TAG-07 前缀更新"
    );
    assert_eq!(read(&root, "keep.md"), "#无关 标签\n", "无关文件逐字节不变");

    // 改写前已备份全部 4 个文件（骨架把备份存成 `<i>.bak`，目录名沿用共享骨架的 `link-rewrite-<stamp>`，
    // 见报告的"已知遗留"）。
    let backup = root
        .join(kp_domain::vault_paths::INTERNAL_DIR)
        .join(kp_domain::vault_paths::BACKUP_DIR);
    let backed = walkdir(&backup);
    assert_eq!(backed.len(), 4, "备份里应有 4 个文件（实得 {backed:?}）");

    reindex(&state);
    let norms = all_norms(&state);
    assert!(
        norms.contains(&"new/name".to_string()),
        "树更新为新名：{norms:?}"
    );
    assert!(
        norms.contains(&"new/name/深".to_string()),
        "子孙标签也更新：{norms:?}"
    );
    assert_eq!(
        tag_notes_impl(
            &state,
            TagNotesArgs {
                tag_norm: "new/name".into(),
                include_children: true
            }
        )
        .expect("新标签的笔记")
        .len(),
        4,
        "4 篇都归到新标签下"
    );
}

/// 简单列目录（递归；测试辅助，避免为一个断言引入依赖）。
fn walkdir(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let entries = match std::fs::read_dir(&cur) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.to_string_lossy().to_string());
            }
        }
    }
    out
}

/// 预览是**一次性**的（第二次用同一个 preview_id 必须失败，且不再改动任何文件）。
#[test]
fn preview_is_one_shot() {
    let (_v, state) = fixture(&[("a.md", "#old 甲\n")]);
    let p = tag_rename_preview_impl(&state, "old".into(), "new".into()).expect("预览");
    tag_rename_apply_impl(&state, "old".into(), "new".into(), p.preview_id.clone()).expect("首次");
    let err = tag_rename_apply_impl(&state, "old".into(), "new".into(), p.preview_id.clone())
        .expect_err("第二次必须失败");
    assert!(
        format!("{err:?}").contains("PreviewExpired"),
        "实得：{err:?}"
    );
}

/// **FR-TAG-08**：循环（新名落在旧标签之下）在**预览阶段**就被拒绝 ⇒ 磁盘零改动、不产生 preview_id。
#[test]
fn fr_tag_08_cycle_is_rejected_before_any_write() {
    let (_v, state) = fixture(&[("a.md", "#old 甲\n")]);
    let err = tag_rename_preview_impl(&state, "old".into(), "old/child".into())
        .expect_err("循环必须被拒");
    assert!(
        format!("{err:?}").contains("InvalidFilename"),
        "实得：{err:?}"
    );
    assert_eq!(read(&root_of(&state), "a.md"), "#old 甲\n", "磁盘零改动");
    assert!(all_norms(&state).contains(&"old".to_string()));
}

/// 预览与执行参数不一致 ⇒ 中止（防止"改了输入再点确认"用错规格改写文件）。
#[test]
fn apply_rejects_mismatched_args() {
    let (_v, state) = fixture(&[("a.md", "#old 甲\n")]);
    let p = tag_rename_preview_impl(&state, "old".into(), "new".into()).expect("预览");
    let err = tag_rename_apply_impl(
        &state,
        "old".into(),
        "别的名字".into(),
        p.preview_id.clone(),
    )
    .expect_err("参数不一致必须失败");
    assert!(
        format!("{err:?}").contains("RewriteFailed"),
        "实得：{err:?}"
    );
    assert_eq!(read(&root_of(&state), "a.md"), "#old 甲\n", "磁盘零改动");
}

/// **陈旧检测**：预览之后文件被外部改动 ⇒ 整体中止、**磁盘零改动**（AC-TAG-02 的"不得出现中间状态"）。
#[test]
fn apply_aborts_when_a_file_changed_after_preview() {
    let (_v, state) = fixture(&[("a.md", "#old 甲\n"), ("b.md", "#old 乙\n")]);
    let p = tag_rename_preview_impl(&state, "old".into(), "new".into()).expect("预览");
    std::fs::write(root_of(&state).join("b.md"), "#old 乙 被外部改过\n").expect("外部改动");
    let err = tag_rename_apply_impl(&state, "old".into(), "new".into(), p.preview_id.clone())
        .expect_err("必须中止");
    assert!(
        format!("{err:?}").contains("RewriteFailed"),
        "实得：{err:?}"
    );
    assert_eq!(read(&root_of(&state), "a.md"), "#old 甲\n", "a 不能被动过");
    assert_eq!(
        read(&root_of(&state), "b.md"),
        "#old 乙 被外部改过\n",
        "b 保持外部改动"
    );
}

/// **AC-TAG-02 末段（全部回滚）**：批量写到一半失败时，**已写的文件必须还原**。
///
/// 注入方式：计划里第二个目标路径建成**目录** ⇒ 原子写必然失败（无法用文件替换目录）；
/// 断言第一个文件已还原成原文、且没有任何新内容留在磁盘上。这条钉的是与标签改名**共用**的落地骨架
/// （`kp_domain::link_rewrite_apply::apply`）的"全有或全无"。
#[test]
fn ac_tag_02_mid_batch_failure_restores_already_written_files() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("ok.md"), "#old 甲\n").expect("写 ok");
    std::fs::create_dir(root.join("blocked.md")).expect("建目录占位");
    let plan = kp_domain::link_rewrite_apply::RewritePlan {
        changes: vec![
            kp_domain::link_rewrite_apply::FileChange {
                rel_path: "ok.md".into(),
                old_content: "#old 甲\n".into(),
                new_content: "#new 甲\n".into(),
                hits: 1,
            },
            kp_domain::link_rewrite_apply::FileChange {
                rel_path: "blocked.md".into(),
                old_content: "#old 乙\n".into(),
                new_content: "#new 乙\n".into(),
                hits: 1,
            },
        ],
        skipped: vec![],
    };
    let backup = root
        .join(kp_domain::vault_paths::INTERNAL_DIR)
        .join(kp_domain::vault_paths::BACKUP_DIR);
    let err = kp_domain::link_rewrite_apply::apply(&root, &plan, &backup)
        .expect_err("第二个目标写不进去 ⇒ 整批必须失败");
    let _ = err;
    assert_eq!(
        std::fs::read_to_string(root.join("ok.md")).expect("读 ok"),
        "#old 甲\n",
        "已写的第一个文件必须被还原（AC-TAG-02：任一文件失败则全部回滚）"
    );
}

/// **AC-TAG-02 末句：旧标签若无其他子孙，则从标签表中消失**（D-41 修好后成立）。
///
/// 曾经不成立：索引重建**只写不删**，`old/name` 会作为"幽灵标签"留在 `tag` 表里（`tag_tree` 会列出它，
/// 点进去却是空列表）。裁定后改为"重建收尾清理无 `file_tag` 引用的 tag 行"（PRD 勘误 **D-41**），
/// 本用例现在断言验收原文的结论。
#[test]
fn ac_tag_02_old_tag_disappears_after_rename_and_rebuild() {
    let (_v, state) = fixture(&[("a.md", "#old/name 甲\n"), ("b.md", "#old/name 乙\n")]);
    let p = tag_rename_preview_impl(&state, "old/name".into(), "new/name".into()).expect("预览");
    tag_rename_apply_impl(
        &state,
        "old/name".into(),
        "new/name".into(),
        p.preview_id.clone(),
    )
    .expect("落地");
    reindex(&state);

    let pool = state.index_db().expect("索引库");
    let count = |sql: &str| -> i64 {
        pool.with_reader(|c| {
            c.query_row(sql, [], |r| r.get(0))
                .map_err(|e| kp_domain::error::AppError::db_with("统计", &e))
        })
        .expect("统计")
    };
    assert_eq!(
        count("SELECT count(*) FROM tag WHERE norm = 'old/name'"),
        0,
        "旧标签行必须消失"
    );
    assert_eq!(
        count("SELECT count(*) FROM tag WHERE norm = 'new/name'"),
        1,
        "新标签行在"
    );
    assert!(
        !all_norms(&state).contains(&"old/name".to_string()),
        "tag_tree 不应再列出旧标签（面板里不该有幽灵条目）：{:?}",
        all_norms(&state)
    );
}
