//! 改写域命令的测试（DEBT-14 的做法：命令体已抽成接收 \`&AppState\` 的函数，可直接调用）。
//!
//! 覆盖门禁 15b 关心的行为面：两阶段（预览 → 一次性执行）、TTL、回滚（含撤销改名）、
//! 歧义消解的"只改一处"、以及各错误路径。

use std::sync::Arc;

use crate::commands::link::{
    link_ambiguous_list_impl, link_backlinks_impl, link_dangling_list_impl,
};
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

/// **AC-LINK-04 的 10 条场景（端到端）**：
/// 知识库里有 10 条 `[[note]]`、而 `note.md` 在两个目录下各有一个（2 个候选）→
/// 用户为**其中一条**指定目标 → **只有那一条**被改写为 `[[f2/note]]`，其余 9 条保持 `[[note]]`
/// 且**仍是歧义**（重新索引后 ref_count 从 10 降到 9），并且源文件已备份（FR-FILE-21）。
#[test]
fn ac_link_04_ten_ambiguous_links_specify_one() {
    let mut src = String::from("# 来源\n");
    for i in 0..10 {
        src.push_str(&format!("第 {i} 行见 [[note]]\n"));
    }
    let (dir, state) = fixture(&[
        ("f1/note.md", "# 候选一\n"),
        ("f2/note.md", "# 候选二\n"),
        ("src.md", src.as_str()),
    ]);
    let root = dir.path();

    // ① 歧义清单：1 组、10 处引用、2 个候选、10 个 link_id
    let page = link_ambiguous_list_impl(&state).expect("歧义清单");
    assert_eq!(page.total, 1, "同目标名归一组");
    let item = &page.items[0];
    assert_eq!(item.target_ref, "note");
    assert_eq!(item.ref_count, 10, "10 条歧义链接");
    assert_eq!(
        item.candidates,
        vec!["f1/note.md", "f2/note.md"],
        "2 个候选"
    );
    assert_eq!(item.link_ids.len(), 10, "每条都要有 id 才能逐条指定");

    // ② 为**其中一条**（第 3 条）指定 f2/note.md
    let chosen = item.link_ids[2];
    let result = link_resolve_ambiguous_impl(&state, chosen, "f2/note.md".into()).expect("消解");
    assert_eq!(result.file_count, 1, "只改一个文件");
    assert_eq!(result.span_count, 1, "只改一处");

    // ③ 源文件：恰好一条变成 [[f2/note]]，其余 9 条保持 [[note]]
    let after = read(root, "src.md");
    assert_eq!(after.matches("[[f2/note]]").count(), 1, "只改指定那一条");
    assert_eq!(after.matches("[[note]]").count(), 9, "其余 9 条不动");
    assert!(
        after.contains("第 2 行见 [[f2/note]]"),
        "改的是第 3 行（0-based 第 2 条）"
    );

    // ④ 备份必须产生（AC-LINK-04 的"已备份"）
    let backup_root = root.join(".knowlpad/backup");
    assert!(backup_root.exists(), "改写前必须备份");

    // ⑤ 重新索引后：歧义只剩 9 条，且 f2/note 有了这条反链（闭环）
    let pool = state.index_db().expect("索引库");
    crate::index_engine::full_index(&pool, root, |_, _| {}).expect("重新索引");
    let page2 = link_ambiguous_list_impl(&state).expect("歧义清单");
    assert_eq!(page2.items[0].ref_count, 9, "被指定的那条已不再是歧义");
    let back = link_backlinks_impl(&state, "f2/note.md", true).expect("反链");
    assert!(
        back.iter().any(|g| g.src_rel_path == "src.md"),
        "f2/note.md 应拿到来自 src.md 的反链"
    );
}

/// **AC-LINK-03 的计数断言（端到端）**：重命名并改写后，**反链条数必须保持**且**不得出现死链**。
///
/// 场景：300 篇笔记各有一条 `[[A]]` → 把 `A.md` 改名为 `B.md`（联合操作，FR-FILE-22）→
/// 重新索引后断言：① `B.md` 的反链仍是 **300 组**（每篇一处）；② 悬空清单为**空**（没有死链）；
/// ③ 歧义清单为空；④ 旧名 `A.md` 不再有任何反链。
#[test]
fn ac_link_03_rename_keeps_all_backlinks_and_leaves_no_dead_links() {
    let mut files: Vec<(String, String)> = vec![("A.md".into(), "# 甲\n".into())];
    for i in 0..300 {
        files.push((
            format!("notes/n{i:03}.md"),
            format!("# 笔记 {i}\n\n见 [[A]]\n"),
        ));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (dir, state) = fixture(&refs);
    let root = dir.path();

    // ① 改名前的反链：300 组
    let before = link_backlinks_impl(&state, "A.md", true).expect("反链");
    assert_eq!(before.len(), 300, "300 篇各有一条反链");
    assert_eq!(before.iter().map(|g| g.link_count).sum::<usize>(), 300);

    // ② 改名 + 改写（一次可回滚操作）
    let rename = RenameSpec {
        from: "A.md".into(),
        to: "B.md".into(),
    };
    let preview =
        link_rewrite_preview_impl(&state, "A.md".into(), "B.md".into(), Some(rename.clone()))
            .expect("预览");
    assert_eq!(preview.file_count, 300, "300 个文件都要改写");
    assert_eq!(preview.span_count, 300, "共 300 处");
    link_rewrite_apply_impl(&state, preview.preview_id.clone(), Some(rename)).expect("执行");
    assert!(root.join("B.md").exists(), "文件已改名");

    // ③ 重新索引后：反链仍 300 组、无死链、无歧义、旧名无残留
    let pool = state.index_db().expect("索引库");
    crate::index_engine::full_index(&pool, root, |_, _| {}).expect("重新索引");
    let after = link_backlinks_impl(&state, "B.md", true).expect("反链");
    assert_eq!(after.len(), 300, "改名后反链条数必须保持（AC-LINK-03）");
    assert_eq!(
        after.iter().map(|g| g.link_count).sum::<usize>(),
        300,
        "总数也不能少"
    );
    assert!(
        after
            .iter()
            .all(|g| g.items.iter().all(|i| i.snippet.is_some())),
        "每条反链都还能取到片段，说明指向的是真实文件"
    );
    let dangling = link_dangling_list_impl(&state).expect("悬空");
    assert_eq!(dangling.total, 0, "改名 + 改写后不得留下死链");
    let ambiguous = link_ambiguous_list_impl(&state).expect("歧义");
    assert_eq!(ambiguous.total, 0, "也不应变成歧义");
    let old = link_backlinks_impl(&state, "A.md", true).expect("旧名反链");
    assert!(old.is_empty(), "旧名不应再有反链");
}
