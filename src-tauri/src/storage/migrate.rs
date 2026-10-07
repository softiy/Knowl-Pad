//! 全局库版本化迁移（技术方案 §4.5；PRD MIG-02 / MIG-03 / MIG-05）。
//!
//! 纪律：**已发布的迁移脚本只增不改**——修改历史脚本会让不同用户的库处于不同实际状态。

use kp_domain::error::AppError;
use rusqlite::Connection;

/// 一次 schema 迁移。
pub struct Migration {
    pub version: i32,
    pub description: &'static str,
    pub up: fn(&Connection) -> Result<(), AppError>,
}

/// 有序迁移表：新版本追加在末尾，禁止修改已发布条目。
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    description: "初始 schema：meta / vault / preference / vault_state",
    up: m001_init,
}];

/// 当前软件支持的最高 schema 版本。
pub fn latest_version() -> i32 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

/// 读取库内 schema 版本。空库（无 meta 表或无记录）视为 0。
pub fn schema_version(conn: &Connection) -> Result<i32, AppError> {
    let has_meta: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |row| row.get(0),
        )
        .map_err(map_err)?;
    if has_meta == 0 {
        return Ok(0);
    }
    match conn.query_row(
        "SELECT value FROM meta WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    ) {
        Ok(raw) => raw.parse::<i32>().map_err(|err| {
            tracing::warn!(error = %err, "版本号非法");
            AppError::db("读取数据库版本")
        }),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(err) => Err(map_err(err)),
    }
}

/// 执行迁移并返回迁移后的版本号。
///
/// - 每个迁移**独立事务**（MIG-02）：失败只回滚当前迁移，已完成的保留；
/// - 库版本高于软件支持时**拒绝**（MIG-03），不静默损坏。
pub fn migrate(conn: &mut Connection) -> Result<i32, AppError> {
    let target = latest_version();
    let applied = migrate_with(conn, MIGRATIONS)?;
    // 内部一致性：迁移表与 latest_version() 必须描述同一目标版本
    debug_assert_eq!(applied, target, "迁移表与 latest_version() 不一致");
    Ok(applied)
}

/// 用给定的迁移表执行迁移。
///
/// 生产路径固定使用 MIGRATIONS；此函数独立出来是为了让「升级路径」可被测试
/// （用测试专用的第二版迁移验证 V1 → V2 的增量升级与数据保留）。
pub fn migrate_with(conn: &mut Connection, migrations: &[Migration]) -> Result<i32, AppError> {
    let current = schema_version(conn)?;
    let latest = migrations.last().map(|m| m.version).unwrap_or(0);
    if current > latest {
        return Err(AppError::DbError(format!(
            "数据库版本 v{current} 高于当前软件支持的 v{latest}，请升级 Knowl Pad"
        )));
    }
    for migration in migrations.iter().filter(|m| m.version > current) {
        let tx = conn.transaction().map_err(map_err)?;
        (migration.up)(&tx)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?1)",
            [migration.version.to_string()],
        )
        .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        tracing::info!(
            version = migration.version,
            description = migration.description,
            "全局库迁移完成"
        );
    }
    schema_version(conn)
}

fn m001_init(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(DDL_V1).map_err(map_err)
}

/// 全局库初始 DDL。**权威出处：PRD §3.3**——此处不得自行增删字段。
pub const DDL_V1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS vault (
    id           INTEGER PRIMARY KEY,
    abs_path     TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    last_opened  INTEGER,
    pinned       INTEGER NOT NULL DEFAULT 0,
    trust_level  TEXT    NOT NULL DEFAULT 'trusted'
);

CREATE TABLE IF NOT EXISTS preference (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS vault_state (
    vault_id INTEGER NOT NULL REFERENCES vault(id) ON DELETE CASCADE,
    key      TEXT    NOT NULL,
    value    TEXT    NOT NULL,
    PRIMARY KEY (vault_id, key)
);
"#;

fn map_err(err: rusqlite::Error) -> AppError {
    tracing::warn!(error = %err, "数据库迁移失败");
    AppError::db("执行数据库迁移")
}

/// 迁移前备份（**DEBT-24①**）：在改动 schema 之前，把当前库一致地导出到同目录的 `.bak` 文件。
///
/// 用 `VACUUM INTO` 而不是复制文件：它由 SQLite 自己产出**事务一致**的快照，
/// 且不关心 WAL/`-shm` 是否落盘（直接拷贝有可能拿到半写状态）。
///
/// 只在**真正的升级**（`from_version > 0` 且低于目标）时调用 —— 新建库无需备份。
pub fn backup_before_migrate(
    conn: &Connection,
    db_path: &std::path::Path,
    from_version: i32,
) -> Result<std::path::PathBuf, AppError> {
    let file_name = db_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "global.db".to_string());
    let base = db_path.with_file_name(format!("{file_name}.bak-v{from_version}"));
    // 已有同名备份时不覆盖：追加时间戳，避免把上一次的回滚点冲掉
    let target = if base.exists() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        db_path.with_file_name(format!("{file_name}.bak-v{from_version}-{nanos}"))
    } else {
        base
    };
    conn.execute(
        "VACUUM INTO ?1",
        rusqlite::params![target.to_string_lossy()],
    )
    .map_err(|_| AppError::db("迁移前备份数据库"))?;
    tracing::info!(backup = %target.display(), from_version, "迁移前已备份全局库");
    Ok(target)
}

#[cfg(test)]
mod tests {
    /// **DEBT-24① 回归**：备份是一致快照，且不覆盖上一次的回滚点。
    #[test]
    fn debt_24_backup_creates_consistent_copy() {
        let dir = tempfile::tempdir().expect("临时目录");
        let path = dir.path().join("global.db");
        let conn = rusqlite::Connection::open(&path).expect("建库");
        conn.execute_batch("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (42);")
            .expect("建表");
        let backup = backup_before_migrate(&conn, &path, 1).expect("备份应成功");
        assert!(backup.exists(), "备份文件应存在");
        let reader = rusqlite::Connection::open(&backup).expect("备份应可打开");
        let value: i64 = reader
            .query_row("SELECT a FROM t", [], |r| r.get(0))
            .expect("备份内应能读回数据（一致快照）");
        assert_eq!(value, 42);
        let again = backup_before_migrate(&conn, &path, 1).expect("第二次备份应成功");
        assert_ne!(again, backup, "同名备份已存在时应追加时间戳，不覆盖");
        assert!(backup.exists(), "上一次的回滚点必须保留");
    }

    use super::*;
    use crate::storage::pragma;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().expect("内存库可打开");
        pragma::apply(&conn).expect("PRAGMA 应成功");
        conn
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0
    }

    #[test]
    fn fresh_db_migrates_to_latest() {
        let mut c = conn();
        assert_eq!(schema_version(&c).expect("版本查询应成功"), 0);
        let version = migrate(&mut c).expect("迁移应成功");
        assert_eq!(version, latest_version());
        for t in ["meta", "vault", "preference", "vault_state"] {
            assert!(table_exists(&c, t), "建表后应存在 {t}");
        }
    }

    #[test]
    fn upgrade_path_preserves_data() {
        // 清单 #7：构造旧库 → 迁移 → 断言（V1 → V2 增量升级，既有数据不丢）
        fn m002_test_only(conn: &Connection) -> Result<(), AppError> {
            conn.execute_batch("CREATE TABLE IF NOT EXISTS test_only(id INTEGER PRIMARY KEY)")
                .map_err(map_err)
        }
        let mut c = conn();
        assert_eq!(migrate(&mut c).expect("V1 迁移应成功"), latest_version());

        // 写入 V1 数据，随后用测试专用迁移表升到 V2
        c.execute(
            "INSERT INTO vault(abs_path, display_name) VALUES('/tmp/v1', 'V1 库')",
            [],
        )
        .expect("写入应成功");
        let v2 = [
            // 字段逐个复制（Migration 非 Copy，不能直接从切片移出）
            Migration {
                version: MIGRATIONS[0].version,
                description: MIGRATIONS[0].description,
                up: MIGRATIONS[0].up,
            },
            Migration {
                version: 2,
                description: "测试专用：验证增量升级路径",
                up: m002_test_only,
            },
        ];
        let version = migrate_with(&mut c, &v2).expect("V2 迁移应成功");
        assert_eq!(version, 2);
        assert!(table_exists(&c, "test_only"), "V2 应新建 test_only 表");
        let name: String = c
            .query_row(
                "SELECT display_name FROM vault WHERE abs_path='/tmp/v1'",
                [],
                |row| row.get(0),
            )
            .expect("V1 数据应保留");
        assert_eq!(name, "V1 库", "升级不得丢失既有数据");
        assert_eq!(migrate_with(&mut c, &v2).expect("重复迁移应成功"), 2);
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut c = conn();
        migrate(&mut c).expect("首次迁移应成功");
        let again = migrate(&mut c).expect("重复迁移应成功且无副作用");
        assert_eq!(again, latest_version());
    }

    #[test]
    fn downgrade_is_rejected() {
        // MIG-03：库版本高于软件支持时必须拒绝，而不是静默损坏
        let mut c = conn();
        migrate(&mut c).expect("迁移应成功");
        c.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', '99')",
            [],
        )
        .expect("写入高版本号应成功");
        let err = migrate(&mut c).expect_err("高版本库必须拒绝迁移");
        assert!(
            err.to_string().contains("高于"),
            "错误信息应说明版本过高：{err}"
        );
    }

    #[test]
    fn foreign_keys_cascade_works() {
        // foreign_keys 未开启时删除 vault 会留下孤立 vault_state
        let mut c = conn();
        migrate(&mut c).expect("迁移应成功");
        c.execute(
            "INSERT INTO vault(id, abs_path, display_name) VALUES(1, '/tmp/v', 'v')",
            [],
        )
        .expect("插入 vault 应成功");
        c.execute(
            "INSERT INTO vault_state(vault_id, key, value) VALUES(1, 'layout', '{}')",
            [],
        )
        .expect("插入 vault_state 应成功");
        c.execute("DELETE FROM vault WHERE id = 1", [])
            .expect("删除 vault 应成功");
        let left: i64 = c
            .query_row("SELECT count(*) FROM vault_state", [], |row| row.get(0))
            .expect("统计应成功");
        assert_eq!(left, 0, "外键级联应清理 vault_state");
    }

    #[test]
    fn vault_abs_path_is_unique() {
        let mut c = conn();
        migrate(&mut c).expect("迁移应成功");
        let insert = |path: &str| {
            c.execute(
                "INSERT INTO vault(abs_path, display_name) VALUES(?1, 'n')",
                [path],
            )
        };
        insert("/tmp/a").expect("首次插入应成功");
        assert!(insert("/tmp/a").is_err(), "同一路径重复注册必须失败");
    }
}
