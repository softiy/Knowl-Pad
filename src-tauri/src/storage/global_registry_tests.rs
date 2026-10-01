//! global.rs 的 registry_tests 测试（CODE-11：测试位于独立文件）。

use super::*;

fn pool() -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let pool = open(dir.path()).expect("打开全局库应成功");
    (dir, pool)
}

#[test]
fn upsert_is_idempotent_and_keeps_display_name() {
    let (_dir, pool) = pool();
    let id1 = upsert_vault(&pool, "/vault/a", "A").expect("注册应成功");
    let id2 = upsert_vault(&pool, "/vault/a", "A-改名前的目录名").expect("重复注册应成功");
    assert_eq!(id1, id2, "同一路径必须复用同一 id");
    rename_vault(&pool, id1, "我的知识库").expect("重命名应成功");
    upsert_vault(&pool, "/vault/a", "目录名").expect("再次打开应成功");
    let rows = list_vaults(&pool).expect("列表应成功");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].display_name, "我的知识库",
        "重开不得覆盖用户改的显示名"
    );
    assert!(rows[0].last_opened.is_some());
}

#[test]
fn duplicate_path_error_is_user_facing_chinese() {
    // ERR-02 回归：relocate 到已被占用的路径会撞 UNIQUE 约束，
    // 用户可见 message 必须是中文可操作文案，**不得**出现 SQL 原文
    let (_dir, pool) = pool();
    upsert_vault(&pool, "/vault/a", "A").expect("注册 A");
    let b = upsert_vault(&pool, "/vault/b", "B").expect("注册 B");
    let err = relocate_vault(&pool, b, "/vault/a").expect_err("重复路径必须失败");
    let message = err.to_string();
    assert_eq!(err.code(), "E_DB_ERROR");
    for leak in ["UNIQUE", "constraint", "sqlite", "SQL"] {
        assert!(
            !message.contains(leak),
            "用户可见 message 不得含技术细节 {leak}：{message}"
        );
    }
    assert!(
        message
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
        "message 必须是中文：{message}"
    );
}

#[test]
fn list_orders_pinned_first_then_recent() {
    let (_dir, pool) = pool();
    let a = upsert_vault(&pool, "/vault/a", "A").expect("注册 A");
    let b = upsert_vault(&pool, "/vault/b", "B").expect("注册 B");
    let c = upsert_vault(&pool, "/vault/c", "C").expect("注册 C");
    pool.with_writer(move |conn| {
        conn.execute("UPDATE vault SET pinned = 1 WHERE id = ?1", [c])
            .map_err(map_err)?;
        conn.execute("UPDATE vault SET last_opened = 200 WHERE id = ?1", [a])
            .map_err(map_err)?;
        conn.execute("UPDATE vault SET last_opened = 100 WHERE id = ?1", [b])
            .map_err(map_err)?;
        Ok(())
    })
    .expect("排序准备应成功");
    let rows = list_vaults(&pool).expect("列表应成功");
    assert_eq!(rows[0].id, c, "置顶应排最前");
    assert_eq!(rows[1].id, a, "其次按最近打开倒序");
    assert_eq!(rows[2].id, b);
}

#[test]
fn remove_only_deletes_registry_row() {
    // AC-VAULT-04：移除注册记录不得删除磁盘文件——本测试用真实目录验证
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let vault_dir = dir.path().join("vault-a");
    std::fs::create_dir_all(&vault_dir).expect("应可创建 Vault 目录");
    let note = vault_dir.join("note.md");
    std::fs::write(&note, "# 内容").expect("应可写入笔记");

    let (_cfg, pool) = pool();
    let id = upsert_vault(&pool, &vault_dir.to_string_lossy(), "A").expect("注册应成功");
    assert!(remove_vault(&pool, id).expect("移除应成功"));
    assert!(
        list_vaults(&pool).expect("列表应成功").is_empty(),
        "记录应被删除"
    );
    assert!(note.exists(), "磁盘文件必须保持不变");
    assert!(
        !remove_vault(&pool, id).expect("重复移除应成功"),
        "重复移除应返回 false"
    );
}

#[test]
fn relocate_updates_path() {
    let (_dir, pool) = pool();
    let id = upsert_vault(&pool, "/old/path", "V").expect("注册应成功");
    relocate_vault(&pool, id, "/new/path").expect("重新定位应成功");
    let rows = list_vaults(&pool).expect("列表应成功");
    assert_eq!(rows[0].abs_path, "/new/path");
}
