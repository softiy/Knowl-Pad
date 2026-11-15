//! AppState 的单元测试（从 state.rs 内联模块搬出：CODE-11 的警告预算已满，
//! 而 `_tests.rs` 这类测试文件本身是豁免的 —— 与 index_watch_tests.rs / error_tests.rs 同一约定）。

use super::*;

fn temp_pool() -> (tempfile::TempDir, Arc<DbPool>) {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let pool = crate::storage::global::open(dir.path()).expect("打开全局库应成功");
    (dir, Arc::new(pool))
}

#[test]
fn index_db_handle_is_settable_and_clearable() {
    let state = AppState::new();
    assert!(state.index_db().is_none(), "初始应为空");
    let (_dir, pool) = temp_pool();
    state.set_index_db(Some(Arc::clone(&pool)));
    assert!(state.index_db().is_some(), "设置后应可读取");
    state.set_index_db(None);
    assert!(state.index_db().is_none(), "清除后应为空");
}

/// SEC-03 / 勘误 D-46：最近一次索引的警告是**覆盖式**状态（只反映本轮），且必须能清空
/// （打开新 Vault / 关闭 Vault / 重建取消时都会清）。
#[test]
fn index_warnings_are_replaceable_and_clearable() {
    let state = AppState::new();
    assert!(state.index_warnings().is_empty(), "初始应无警告");
    state.set_index_warnings(vec!["skip-timeout:超长单行.md".to_string()]);
    assert_eq!(
        state.index_warnings(),
        vec!["skip-timeout:超长单行.md".to_string()]
    );
    // 覆盖而不是追加：只保留本轮结果
    state.set_index_warnings(vec!["skip-oversize:巨大笔记.md".to_string()]);
    assert_eq!(
        state.index_warnings(),
        vec!["skip-oversize:巨大笔记.md".to_string()]
    );
    state.set_index_warnings(Vec::new());
    assert!(state.index_warnings().is_empty(), "必须能清空");
}

#[test]
fn global_db_handle_is_settable() {
    let state = AppState::new();
    assert!(state.global_db().is_none());
    let (_dir, pool) = temp_pool();
    state.set_global_db(pool);
    assert!(state.global_db().is_some());
}
