//! 改写域命令的测试（DEBT-14 的做法：命令体已抽成接收 \`&AppState\` 的函数，可直接调用）。
//!
//! 覆盖门禁 15b 关心的行为面：两阶段（预览 → 一次性执行）、TTL、回滚（含撤销改名）、
//! 歧义消解的"只改一处"、以及各错误路径。

use std::sync::Arc;

use crate::commands::link_resolve::link_resolve_ambiguous_impl;
use crate::commands::link_rewrite::{
    link_rewrite_apply_impl, link_rewrite_preview_impl, RenameSpec,
};
use crate::commands::link_rewrite_store::{
    link_rewrite_rollback_impl, next_id, put_preview, take_preview, StoredPreview,
};
use crate::state::AppState;

/// 临时 Vault + 已建索引的 AppState（命令体要的就是"根 + 索引库"）。
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
    state.set_root(Some(root.clone()));
    state.set_index_db(Some(Arc::new(pool)));
    (dir, state)
}

fn read(root: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).expect("读文件")
}

/// 预览 → 执行：文件被改写、备份产生、operation 已登记。
#[test]
fn preview_then_apply_rewrites_and_registers_operation() {
    let (dir, state) = fixture(&[
        ("A.md", "# 甲\n"),
        ("one.md", "见 [[A]]\n"),
        ("two.md", "也见 [[A|别名]]\n"),
    ]);
    let root = dir.path();
    let preview =
        link_rewrite_preview_impl(&state, "A.md".into(), "B.md".into(), None).expect("预览应成功");
    assert_eq!(preview.file_count, 2, "两个文件各有一处");
    assert_eq!(preview.span_count, 2);
    assert!(!preview.preview_id.is_empty(), "必须给出 preview_id");

    let result =
        link_rewrite_apply_impl(&state, preview.preview_id.clone(), None).expect("执行应成功");
    assert_eq!(result.file_count, 2);
    assert_eq!(result.span_count, 2);
    assert!(read(root, "one.md").contains("[[B]]"));
    assert!(read(root, "two.md").contains("[[B|别名]]"), "别名保留");
    assert!(
        std::path::Path::new(&format!("{}/.knowlpad/backup", root.display())).exists(),
        "备份目录必须产生（FR-FILE-21 ①）"
    );
    assert!(
        result.operation_id.starts_with("op"),
        "operation_id 应有前缀"
    );
}

/// **一次性消费**：同一个 preview_id 第二次执行必须 E_PREVIEW_EXPIRED（PRD §5.3.3）。
#[test]
fn apply_is_one_shot() {
    let (_d, state) = fixture(&[("A.md", "# 甲\n"), ("one.md", "见 [[A]]\n")]);
    let p = link_rewrite_preview_impl(&state, "A.md".into(), "B.md".into(), None).expect("预览");
    link_rewrite_apply_impl(&state, p.preview_id.clone(), None).expect("第一次应成功");
    let err =
        link_rewrite_apply_impl(&state, p.preview_id.clone(), None).expect_err("第二次必须失败");
    assert_eq!(err.0.code(), "E_PREVIEW_EXPIRED");
}

/// 未知 preview_id 同样是 E_PREVIEW_EXPIRED（不区分"没预览过"与"已消费"，PRD 只给一个码）。
#[test]
fn unknown_preview_id_is_expired() {
    let (_d, state) = fixture(&[("A.md", "# 甲\n")]);
    let err =
        link_rewrite_apply_impl(&state, "p-does-not-exist".into(), None).expect_err("必须失败");
    assert_eq!(err.0.code(), "E_PREVIEW_EXPIRED");
}

/// TTL：过期的预览必须取不到（技术方案 §6.1.1 的 10 分钟）。
#[test]
fn expired_preview_is_dropped() {
    let (_d, state) = fixture(&[("A.md", "# 甲\n")]);
    let id = next_id("ptest");
    put_preview(
        &state,
        id.clone(),
        StoredPreview {
            created_at_ms: 0, // 1970：必然过期
            vault_root: std::path::PathBuf::from("C:/whatever"),
            from_ref: "A".into(),
            to_ref: "B".into(),
            rename: None,
            plan: kp_domain::link_rewrite_apply::RewritePlan::default(),
        },
    );
    assert!(take_preview(&state, &id).is_none(), "过期预览不得返回");
    assert!(take_preview(&state, &id).is_none(), "取走后再取也没有");
}

/// 回滚：把改写过的文件恢复成原文（备份就是为它存在的，AC-FILE-02）。
#[test]
fn rollback_restores_rewritten_file() {
    let (dir, state) = fixture(&[("A.md", "# 甲\n"), ("one.md", "见 [[A]]\n")]);
    let root = dir.path();
    let p = link_rewrite_preview_impl(&state, "A.md".into(), "B.md".into(), None).expect("预览");
    let r = link_rewrite_apply_impl(&state, p.preview_id.clone(), None).expect("执行");
    assert!(read(root, "one.md").contains("[[B]]"));
    link_rewrite_rollback_impl(&state, r.operation_id.clone()).expect("回滚应成功");
    assert_eq!(read(root, "one.md"), "见 [[A]]\n", "必须恢复成原文");
}

/// 回滚**含改名**的操作：先恢复链接、再把文件改回原名（技术方案 §6.1.3 步骤 4 的顺序）。
#[test]
fn rollback_undoes_rename_too() {
    let (dir, state) = fixture(&[("A.md", "# 甲\n"), ("one.md", "见 [[A]]\n")]);
    let root = dir.path();
    let rename = RenameSpec {
        from: "A.md".into(),
        to: "B.md".into(),
    };
    let p = link_rewrite_preview_impl(&state, "A.md".into(), "B.md".into(), Some(rename.clone()))
        .expect("预览");
    let r = link_rewrite_apply_impl(&state, p.preview_id.clone(), Some(rename)).expect("执行");
    assert!(root.join("B.md").exists(), "文件应已改名");
    assert!(read(root, "one.md").contains("[[B]]"));
    link_rewrite_rollback_impl(&state, r.operation_id.clone()).expect("回滚应成功");
    assert!(root.join("A.md").exists(), "文件必须改回原名");
    assert!(!root.join("B.md").exists(), "不得停在新名上");
    assert_eq!(read(root, "one.md"), "见 [[A]]\n", "链接也要恢复");
}

/// 未打开 Vault（没有根）时报 E_VAULT_NOT_OPEN，而不是 panic。
#[test]
fn preview_without_vault_errors() {
    let state = AppState::new();
    let err =
        link_rewrite_preview_impl(&state, "A".into(), "B".into(), None).expect_err("必须失败");
    assert_eq!(err.0.code(), "E_VAULT_NOT_OPEN");
}

/// 歧义消解：只改指定那一处，并把目标写成**完整相对路径**（FR-LINK-22 / AC-LINK-05）。
#[test]
fn resolve_ambiguous_rewrites_only_that_occurrence() {
    let (dir, state) = fixture(&[
        ("folder/note.md", "# 候选一\n"),
        ("other/note.md", "# 候选二\n"),
        ("src.md", "首行 [[note]]\n\n第三行 [[note]]\n"),
    ]);
    let root = dir.path();
    // 取一条 ambiguous 链接的 id 与位置
    let pool = state.index_db().expect("索引库");
    let (id, line): (i64, u32) = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT id, line FROM link WHERE status = 'ambiguous' ORDER BY line LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| kp_domain::error::AppError::db(&format!("取歧义链接失败：{e}")))
        })
        .expect("取歧义链接");
    let result = link_resolve_ambiguous_impl(&state, id, "other/note".into()).expect("消解应成功");
    assert_eq!(result.file_count, 1);
    let after = read(root, "src.md");
    assert!(
        after.contains("[[other/note]]"),
        "指定那处应改成完整相对路径：{after}"
    );
    assert_eq!(
        after.matches("[[note]]").count(),
        1,
        "另一处必须原样保留（只改一处）：{after}"
    );
    assert!(line >= 1, "位置来自索引，应为 1-based");
}

/// 不存在的 link_id：报 E_FILE_NOT_FOUND，而不是静默成功。
#[test]
fn resolve_unknown_link_errors() {
    let (_d, state) = fixture(&[("a.md", "# 甲\n")]);
    let err = link_resolve_ambiguous_impl(&state, 999_999, "x/y".into()).expect_err("必须失败");
    assert_eq!(err.0.code(), "E_FILE_NOT_FOUND");
}
