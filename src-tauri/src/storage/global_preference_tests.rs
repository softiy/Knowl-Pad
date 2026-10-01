//! global.rs 的 preference_tests（CODE-11：测试位于独立文件）。

use super::*;

fn pool() -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let pool = open(dir.path()).expect("打开全局库应成功");
    (dir, pool)
}

#[test]
fn last_vault_roundtrip_and_clear() {
    let (_dir, pool) = pool();
    let id = upsert_vault(&pool, "/vault/a", "A").expect("注册应成功");
    assert!(
        last_vault(&pool).expect("读取应成功").is_none(),
        "初始无记录"
    );
    set_last_vault(&pool, id).expect("写入应成功");
    let row = last_vault(&pool).expect("读取应成功").expect("应有记录");
    assert_eq!(row.id, id);
    clear_last_vault(&pool).expect("清除应成功");
    assert!(
        last_vault(&pool).expect("读取应成功").is_none(),
        "清除后不应再有记录"
    );
}

#[test]
fn restore_flag_defaults_to_true_and_can_be_disabled() {
    let (_dir, pool) = pool();
    assert!(
        restore_last_enabled(&pool).expect("读取应成功"),
        "默认应允许恢复"
    );
    set_preference(&pool, PREF_RESTORE_LAST, "false").expect("写入应成功");
    assert!(
        !restore_last_enabled(&pool).expect("读取应成功"),
        "关闭后不应恢复"
    );
}

#[test]
fn last_vault_tolerates_removed_or_invalid_record() {
    let (_dir, pool) = pool();
    let id = upsert_vault(&pool, "/vault/gone", "G").expect("注册应成功");
    set_last_vault(&pool, id).expect("写入应成功");
    remove_vault(&pool, id).expect("移除应成功");
    assert!(
        last_vault(&pool).expect("读取应成功").is_none(),
        "记录已删应返回 None"
    );
    set_preference(&pool, PREF_LAST_VAULT, "not-a-number").expect("写入应成功");
    assert!(
        last_vault(&pool).expect("读取应成功").is_none(),
        "非法格式应被忽略"
    );
}
