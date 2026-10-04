//! vault_state 存储的单元测试（CODE-11：测试位于独立文件）。

use super::*;

fn pool_and_vault() -> (tempfile::TempDir, DbPool, i64) {
    let dir = tempfile::tempdir().expect("配置目录应可创建");
    let pool = crate::storage::global::open(dir.path()).expect("全局库应可打开");
    let vault = crate::storage::global::upsert_vault(&pool, "/vault/a", "A").expect("注册应成功");
    (dir, pool, vault)
}

#[test]
fn write_then_read_round_trip() {
    let (_d, pool, vault) = pool_and_vault();
    assert_eq!(read_state(&pool, vault, "tree.expanded").unwrap(), None);
    write_state(&pool, vault, "tree.expanded", r#"["a","b/c"]"#).unwrap();
    assert_eq!(
        read_state(&pool, vault, "tree.expanded")
            .unwrap()
            .as_deref(),
        Some(r#"["a","b/c"]"#)
    );
}

#[test]
fn write_is_upsert_not_duplicate() {
    let (_d, pool, vault) = pool_and_vault();
    write_state(&pool, vault, "layout", "\"edit\"").unwrap();
    write_state(&pool, vault, "layout", "\"split\"").unwrap();
    assert_eq!(
        read_state(&pool, vault, "layout").unwrap().as_deref(),
        Some("\"split\"")
    );
    assert_eq!(
        read_all(&pool, vault).unwrap().len(),
        1,
        "同一键不得产生多行"
    );
}

#[test]
fn read_all_is_sorted_by_key() {
    let (_d, pool, vault) = pool_and_vault();
    write_state(&pool, vault, "z.key", "1").unwrap();
    write_state(&pool, vault, "a.key", "2").unwrap();
    let all = read_all(&pool, vault).unwrap();
    assert_eq!(
        all.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["a.key", "z.key"]
    );
}

#[test]
fn state_is_isolated_per_vault() {
    let (_d, pool, first) = pool_and_vault();
    let second = crate::storage::global::upsert_vault(&pool, "/vault/b", "B").unwrap();
    write_state(&pool, first, "tree.expanded", r#"["x"]"#).unwrap();
    assert_eq!(
        read_state(&pool, second, "tree.expanded").unwrap(),
        None,
        "不得跨 Vault 串味"
    );
}

#[test]
fn oversized_value_is_rejected_without_writing() {
    let (_d, pool, vault) = pool_and_vault();
    let huge = "x".repeat(MAX_STATE_BYTES + 1);
    let err = write_state(&pool, vault, "tree.expanded", &huge).unwrap_err();
    assert_eq!(err.code(), "E_IO_FAILURE");
    assert_eq!(
        read_state(&pool, vault, "tree.expanded").unwrap(),
        None,
        "超限不得写入"
    );
}
