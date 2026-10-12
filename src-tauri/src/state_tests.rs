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

#[test]
fn global_db_handle_is_settable() {
    let state = AppState::new();
    assert!(state.global_db().is_none());
    let (_dir, pool) = temp_pool();
    state.set_global_db(pool);
    assert!(state.global_db().is_some());
}
