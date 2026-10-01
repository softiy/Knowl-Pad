//! 索引库 schema/签名/丢弃重建的单元测试
//! （CODE-11：测试位于独立文件，不占用实现文件行数上限）

use super::*;
use crate::storage::migrate;

fn temp_vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    std::fs::create_dir_all(dir.path().join(".knowlpad")).expect("应可创建 .knowlpad");
    dir
}

fn insert_file(pool: &DbPool, rel_path: &str) -> i64 {
    // with_writer 的闭包要求 'static，故路径先转为 owned String
    let rel_path = rel_path.to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO file(rel_path, name, stem, ext, kind, size_bytes, mtime_ms, indexed_at)
                 VALUES(?1, 'n.md', 'n', 'md', 'note', 10, 1, 1)",
            [rel_path.as_str()],
        )
        .map_err(map_err)?;
        Ok(conn.last_insert_rowid())
    })
    .expect("插入文件应成功")
}

fn count_files(pool: &DbPool) -> i64 {
    pool.with_reader(|conn| {
        conn.query_row("SELECT count(*) FROM file", [], |row| row.get(0))
            .map_err(map_err)
    })
    .expect("统计应成功")
}

#[test]
fn open_creates_full_schema_and_meta() {
    let dir = temp_vault();
    let pool = open(dir.path()).expect("打开索引库应成功");
    pool.with_reader(|conn| {
        for table in [
            "meta",
            "file",
            "heading",
            "block_id",
            "link",
            "tag",
            "file_tag",
            "file_alias",
            "note_fts",
        ] {
            assert!(table_exists(conn, table)?, "应存在表 {table}");
        }
        // meta 必须写入 PRD §3.2.1 规定的固定键
        for key in [
            "schema_version",
            "index_signature",
            "vault_root",
            "parser_version",
            "built_at",
        ] {
            assert!(read_meta(conn, key)?.is_some(), "meta 应含键 {key}");
        }
        assert_eq!(
            read_meta(conn, "schema_version")?.as_deref(),
            Some(SCHEMA_VERSION.to_string().as_str())
        );
        Ok(())
    })
    .expect("schema 断言应通过");
}

#[test]
fn reopen_with_same_signature_keeps_data() {
    let dir = temp_vault();
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "a.md");
    }
    let pool = open(dir.path()).expect("再次打开应成功");
    assert_eq!(count_files(&pool), 1, "签名一致时不应重建");
}

#[test]
fn signature_mismatch_triggers_drop_and_rebuild() {
    let dir = temp_vault();
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "a.md");
        // 模拟软件升级后签名变化
        pool.with_writer(|conn| set_meta(conn, "index_signature", "stale-signature"))
            .expect("改写签名应成功");
    }
    let pool = open(dir.path()).expect("签名不一致时应重建并成功打开");
    assert_eq!(count_files(&pool), 0, "签名不一致必须丢弃既有索引");
}

#[test]
fn interrupted_rebuild_is_restarted() {
    // FR-SIG-03 / NFR-REL-07：残留重建标记 → 重新开始，绝不把半索引当有效索引
    let dir = temp_vault();
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "a.md");
        pool.with_writer(|conn| set_meta(conn, META_REBUILD_IN_PROGRESS, "1"))
            .expect("写入标记应成功");
    }
    let pool = open(dir.path()).expect("应重新开始重建");
    assert_eq!(count_files(&pool), 0, "未完成重建必须重来");
}

#[test]
fn missing_signature_is_treated_as_untrusted() {
    // #9 回归：meta 存在但缺 index_signature → 必须重建（缺失 ≠ 一致）
    let dir = temp_vault();
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "a.md");
        pool.with_writer(|conn| clear_meta(conn, "index_signature"))
            .expect("删除签名应成功");
    }
    let pool = open(dir.path()).expect("缺签名时应重建并成功打开");
    assert_eq!(count_files(&pool), 0, "缺签名的索引库不可信，必须重建");
}

#[test]
fn rebuild_marker_is_cleared_after_successful_open() {
    // FR-SIG-03 的另一半：成功打开后不得留下「重建进行中」标记
    let dir = temp_vault();
    let pool = open(dir.path()).expect("打开应成功");
    let marker = pool
        .with_reader(|conn| read_meta(conn, META_REBUILD_IN_PROGRESS))
        .expect("读取标记应成功");
    assert!(marker.is_none(), "完成后必须清除重建标记");
}

#[test]
fn fts5_table_is_usable() {
    // 为 M3 铺路：确认 FTS5 表可写入并支持 MATCH 查询与 rowid 对齐
    let dir = temp_vault();
    let pool = open(dir.path()).expect("打开应成功");
    let file_id = insert_file(&pool, "n.md");
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO note_fts(rowid, plain_text) VALUES(?1, ?2)",
            rusqlite::params![file_id, "知识管理 markdown 笔记"],
        )
        .map_err(map_err)?;
        Ok(())
    })
    .expect("写入 FTS 应成功");
    let hits: i64 = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT count(*) FROM note_fts WHERE note_fts MATCH 'markdown'",
                [],
                |row| row.get(0),
            )
            .map_err(map_err)
        })
        .expect("MATCH 查询应成功");
    assert_eq!(hits, 1, "FTS5 应能命中并支持 MATCH");
}

#[test]
fn cascade_delete_removes_children() {
    let dir = temp_vault();
    let pool = open(dir.path()).expect("打开应成功");
    let file_id = insert_file(&pool, "n.md");
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO heading(file_id, level, text, anchor, line, sort_order)
                 VALUES(?1, 1, '标题', '标题', 0, 0)",
            [file_id],
        )
        .map_err(map_err)?;
        conn.execute("DELETE FROM file WHERE id = ?1", [file_id])
            .map_err(map_err)?;
        let left: i64 = conn
            .query_row("SELECT count(*) FROM heading", [], |row| row.get(0))
            .map_err(map_err)?;
        assert_eq!(left, 0, "删除文件应级联清理 heading");
        Ok(())
    })
    .expect("级联断言应通过");
}

#[test]
fn signature_digest_is_deterministic_and_sensitive() {
    let base = IndexSignature::from_parts(1, 0, "v1", "h1", "/vault");
    let same = IndexSignature::from_parts(1, 0, "v1", "h1", "/vault");
    assert_eq!(base.digest, same.digest, "同输入必须同 digest");
    assert_eq!(base.digest.len(), 64, "SHA-256 十六进制应为 64 字符");
    let bumped = IndexSignature::from_parts(2, 0, "v1", "h1", "/vault");
    assert_ne!(base.digest, bumped.digest, "版本变化必须改变 digest");
    assert!(base.diff_reason(&bumped).contains("schema_version"));
}

#[test]
fn index_db_and_global_db_schemas_are_independent() {
    // 索引库不得使用全局库的迁移表（两者策略不同：丢弃重建 vs 版本化迁移）
    let dir = temp_vault();
    let pool = open(dir.path()).expect("打开应成功");
    pool.with_reader(|conn| {
        assert!(
            !table_exists(conn, "vault")?,
            "索引库不应含全局库的 vault 表"
        );
        assert!(migrate::schema_version(conn).unwrap_or(0) >= 0);
        Ok(())
    })
    .expect("断言应通过");
}
