//! 索引库 schema/签名/丢弃重建的单元测试
//! （CODE-11：测试位于独立文件，不占用实现文件行数上限）

use super::*;
use crate::storage::index_rebuild::{ensure_index_db, RebuildOutcome};
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

// ── §4.4 文件级重建机制（M1 落地部分）──────────────────────────────

fn rebuild_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join(".knowlpad")
        .join(crate::storage::index_rebuild::REBUILD_DB_NAME)
}

fn old_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join(".knowlpad")
        .join(crate::storage::index_rebuild::OLD_DB_NAME)
}

#[test]
fn fresh_index_is_built_in_separate_file_then_switched() {
    let dir = temp_vault();
    let db = dir.path().join(".knowlpad/index.db");
    let sig = IndexSignature::current(dir.path());
    let outcome = ensure_index_db(&db, &sig).expect("建库应成功");
    assert_eq!(outcome, RebuildOutcome::Fresh);
    assert!(db.is_file(), "切换后 index.db 应存在");
    assert!(!rebuild_path(dir.path()).exists(), "不得残留 .rebuild");
    assert!(!old_path(dir.path()).exists(), "首次建库不应产生 .old");
}

#[test]
fn matching_signature_leaves_index_untouched() {
    let dir = temp_vault();
    let db = dir.path().join(".knowlpad/index.db");
    let sig = IndexSignature::current(dir.path());
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "keep.md");
    }
    let outcome = ensure_index_db(&db, &sig).expect("判定应成功");
    assert_eq!(outcome, RebuildOutcome::Unchanged, "签名一致不得重建");
    let pool = open(dir.path()).expect("再次打开应成功");
    assert_eq!(count_files(&pool), 1, "既有数据必须保留");
}

#[test]
fn stale_signature_rebuilds_and_reports_reason() {
    let dir = temp_vault();
    let db = dir.path().join(".knowlpad/index.db");
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "gone.md");
        pool.with_writer(|conn| set_meta(conn, "index_signature", "stale"))
            .expect("改写签名应成功");
    }
    let sig = IndexSignature::current(dir.path());
    let outcome = ensure_index_db(&db, &sig).expect("重建应成功");
    match outcome {
        RebuildOutcome::Rebuilt { reason } => {
            assert!(reason.contains("schema_version") || !reason.is_empty());
        }
        other => panic!("应报告已重建，实得 {other:?}"),
    }
    let pool = open(dir.path()).expect("重建后打开应成功");
    assert_eq!(count_files(&pool), 0, "签名不符必须丢弃既有索引");
    assert!(!rebuild_path(dir.path()).exists(), "不得残留 .rebuild");
    assert!(!old_path(dir.path()).exists(), "切换后应删除 .old");
}

#[test]
fn interrupted_rebuild_residue_is_discarded_without_touching_index() {
    // §4.4 步骤 6：残留的 .rebuild 是「上次中断」的证据，应丢弃；**正在服务的 index.db 不受影响**
    let dir = temp_vault();
    {
        let pool = open(dir.path()).expect("首次打开应成功");
        insert_file(&pool, "keep.md");
    }
    let residue = rebuild_path(dir.path());
    std::fs::write(&residue, b"half-built rebuild artifact").expect("应可写残留");
    std::fs::write(dir.path().join(".knowlpad/index.db.rebuild-wal"), b"wal").expect("应可写");

    let pool = open(dir.path()).expect("打开应成功");
    assert!(!residue.exists(), "残留的 .rebuild 必须被丢弃");
    assert!(
        !dir.path().join(".knowlpad/index.db.rebuild-wal").exists(),
        "残留的 -wal 也必须被丢弃"
    );
    assert_eq!(count_files(&pool), 1, "签名一致时既有索引必须完好");
}

#[test]
fn unreadable_index_is_treated_as_untrusted_and_rebuilt() {
    // 索引是可丢弃的派生数据：损坏时重建，而不是让 Vault 打不开
    let dir = temp_vault();
    let db = dir.path().join(".knowlpad/index.db");
    std::fs::write(&db, b"this is not a sqlite database at all").expect("应可写坏文件");
    let pool = open(dir.path()).expect("损坏的索引库应被重建而不是报错");
    assert_eq!(count_files(&pool), 0);
    let sig = IndexSignature::current(dir.path());
    let outcome = ensure_index_db(&db, &sig).expect("判定应成功");
    assert_eq!(outcome, RebuildOutcome::Unchanged, "重建后签名应一致");
}

/// WP3 的核心验证：**中文短语**经 jieba 预分词写入 FTS5（unicode61）后能被 MATCH 命中，
/// 且不相干的词不命中（AC-SEARCH-01 的最小版）。
#[test]
fn fts_roundtrip_matches_chinese_phrase_after_tokenization() {
    let dir = tempfile::tempdir().expect("临时目录");
    let pool = crate::storage::index::open(dir.path()).expect("建索引库");
    let text = kp_domain::tokenize::for_index("知识图谱的力导向布局");
    assert!(!text.is_empty(), "分词结果不应为空");
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO note_fts(rowid, plain_text) VALUES (1, ?1)",
            rusqlite::params![text],
        )
        .map(|_| ())
        .map_err(|_| kp_domain::error::AppError::db("插入 FTS 行"))
    })
    .expect("写入 FTS");

    let hit_expr =
        kp_domain::search::build_match_expr("力导向布局", kp_domain::search::MatchMode::All);
    let hits: i64 = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT count(*) FROM note_fts WHERE note_fts MATCH ?1",
                rusqlite::params![hit_expr],
                |row| row.get(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("查询 FTS"))
        })
        .expect("查询 FTS");
    assert!(hits >= 1, "中文短语应命中（expr = {hit_expr}）");

    let miss_expr =
        kp_domain::search::build_match_expr("量子纠缠退相干", kp_domain::search::MatchMode::All);
    let misses: i64 = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT count(*) FROM note_fts WHERE note_fts MATCH ?1",
                rusqlite::params![miss_expr],
                |row| row.get(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("查询 FTS"))
        })
        .expect("查询 FTS");
    assert_eq!(misses, 0, "不相干的词不应命中（expr = {miss_expr}）");
}

/// 含 FTS5 元字符的输入不得造成语法错误（escape_fts 的作用）。
#[test]
fn fts_special_characters_do_not_error() {
    let dir = tempfile::tempdir().expect("临时目录");
    let pool = crate::storage::index::open(dir.path()).expect("建索引库");
    let expr = kp_domain::search::build_match_expr("a*b (c) ^d", kp_domain::search::MatchMode::All);
    let res: Result<i64, _> = pool.with_reader(|conn| {
        conn.query_row(
            "SELECT count(*) FROM note_fts WHERE note_fts MATCH ?1",
            rusqlite::params![expr],
            |row| row.get(0),
        )
        .map_err(|_| kp_domain::error::AppError::db("查询 FTS"))
    });
    assert!(res.is_ok(), "元字符输入不得导致 SQL 错误：{res:?}");
}

/// **M3 复核 M3-8 的判据回归**：`ready` 必须来自存储层自己的判定，而不是"meta 里有没有记录"
/// —— 后者恒为真（`bootstrap` 开库时无条件 `write_meta`），上一轮就是被这一点推翻的。
#[test]
fn open_with_outcome_reports_unchanged_only_after_an_index_exists() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("a.md"), "# 甲\n").expect("写笔记");

    // 第一次：索引库不存在 → Fresh（此时**尚不可用**，ready 应为 false）
    let (pool, first) = crate::storage::index::open_with_outcome(&root).expect("首次打开");
    assert!(
        matches!(first, RebuildOutcome::Fresh),
        "首次打开应为 Fresh，实际 {first:?}"
    );
    drop(pool);

    // 第二次：索引库已在 → 判定应为 Unchanged（文件被保留）
    let (pool, second) = crate::storage::index::open_with_outcome(&root).expect("再次打开");
    assert!(
        matches!(second, RebuildOutcome::Unchanged),
        "既有索引库应为 Unchanged，实际 {second:?}"
    );

    // 而 meta 是否存在**不能**区分这两种情形 —— 这正是判据被推翻的原因
    let (sig, built) = pool
        .with_reader(|conn| {
            Ok((
                crate::storage::index::read_meta(conn, "index_signature")?,
                crate::storage::index::read_meta(conn, "built_at")?,
            ))
        })
        .expect("读 meta");
    assert!(
        sig.is_some() && built.is_some(),
        "两次打开后 meta 都已存在，所以「meta 存在」不构成判据"
    );
}
