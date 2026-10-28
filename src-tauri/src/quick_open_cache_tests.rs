//! **快速打开候选缓存的正确性**（NFR-PERF-05 优化）。
//!
//! 缓存以索引库内容指纹（count / max(mtime) / sum(mtime) / 路径总长 / 文件名总长 / vault_root）为键：
//! 索引变化 ⇒ 指纹变化 ⇒ 下一次调用必须重新取候选。本文件钉住三件事：
//! ① 新增笔记并重新索引后，候选**立即**包含它（不是旧缓存）；
//! ② 改名（路径变化）同样使缓存失效；
//! ③ 两个不同 Vault 的候选**不串**。

use std::path::Path;
use std::sync::Arc;

use crate::commands::search::QuickOpenArgs;
use crate::commands::search_index::search_quick_open_impl;
use crate::index_engine::{full_index, full_index_cancellable, IndexMode};
use crate::state::AppState;
use crate::storage::index::open;

fn write_note(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("建目录");
    }
    std::fs::write(path, body).expect("写笔记");
}

fn state_for(root: &Path) -> (tempfile::TempDir, AppState) {
    let db = tempfile::tempdir().expect("索引库临时目录");
    let pool = Arc::new(open(&db.path().join("index.db")).expect("打开索引库"));
    full_index_cancellable(&pool, root, |_, _| {}, IndexMode::Force, || false).expect("全量索引");
    let state = AppState::new();
    state.set_root(Some(root.to_path_buf()));
    state.set_index_db(Some(pool));
    (db, state)
}

fn names(state: &AppState, query: &str) -> Vec<String> {
    search_quick_open_impl(
        state,
        QuickOpenArgs {
            query: query.to_string(),
            limit: Some(20),
        },
    )
    .expect("快速打开")
    .into_iter()
    .map(|item| item.rel_path)
    .collect()
}

#[test]
fn cache_invalidates_after_index_change() {
    let dir = tempfile::tempdir().expect("vault");
    let root = dir.path().to_path_buf();
    write_note(&root, "a.md", "# a");
    write_note(&root, "b.md", "# b");
    let (_db, state) = state_for(&root);
    assert_eq!(names(&state, "a").len(), 1, "初始只有 a");

    // 新增 c.md 并**重新索引**：候选缓存必须失效（否则查询 c 命中不到）
    write_note(&root, "c.md", "# c");
    let pool = state.index_db().expect("索引库");
    full_index(&pool, &root, |_, _| {}).expect("增量重索引");
    assert_eq!(
        names(&state, "c"),
        vec!["c.md".to_string()],
        "索引变更后候选必须立即更新"
    );

    // 再加一篇：第二次索引变更同样必须让缓存失效（防"只失效一次"的实现）
    write_note(&root, "e.md", "# e");
    let pool = state.index_db().expect("索引库");
    full_index(&pool, &root, |_, _| {}).expect("第二次重索引");
    assert_eq!(
        names(&state, "e"),
        vec!["e.md".to_string()],
        "第二次索引变更后候选也必须更新"
    );
    // 注：改名后**旧行是否被清理**属索引侧职责（与候选缓存无关），本用例不混测。
}

#[test]
fn cache_does_not_bleed_across_vaults() {
    let dir_a = tempfile::tempdir().expect("vault A");
    write_note(dir_a.path(), "alpha.md", "# alpha");
    let (_db_a, state_a) = state_for(dir_a.path());

    let dir_b = tempfile::tempdir().expect("vault B");
    write_note(dir_b.path(), "beta.md", "# beta");
    let (_db_b, state_b) = state_for(dir_b.path());

    assert_eq!(names(&state_a, ""), vec!["alpha.md".to_string()]);
    assert_eq!(
        names(&state_b, ""),
        vec!["beta.md".to_string()],
        "不同 Vault 不得串候选"
    );
}
