//! SQLite PRAGMA 配置（技术方案 §4.2）。索引库与全局库共用同一套设置。

use kp_domain::error::AppError;
use rusqlite::Connection;

/// 设置后不返回结果行的 PRAGMA。
const PLAIN_PRAGMAS: &[&str] = &[
    "PRAGMA foreign_keys = ON", // 必须显式开启：rusqlite 默认关闭，否则 ON DELETE CASCADE 失效
    "PRAGMA synchronous = NORMAL", // WAL 下 NORMAL 已保证进程崩溃一致性；FULL 会显著拖慢索引
    "PRAGMA busy_timeout = 5000",
    "PRAGMA cache_size = -64000",
    "PRAGMA temp_store = MEMORY",
    "PRAGMA wal_autocheckpoint = 1000",
];

/// 设置后会返回一行结果的 PRAGMA（用 query_row 读取并丢弃结果）。
const QUERY_PRAGMAS: &[&str] = &["PRAGMA journal_mode = WAL"];

/// mmap 为性能优化项：个别平台/文件系统不支持时**只降级不报错**（技术方案 §4.2 标注 Windows 需实测）。
const MMAP_PRAGMA: &str = "PRAGMA mmap_size = 268435456";

/// 应用全部 PRAGMA（幂等，可对同一连接重复调用）。
pub fn apply(conn: &Connection) -> Result<(), AppError> {
    for sql in PLAIN_PRAGMAS {
        conn.execute_batch(sql).map_err(map_err)?;
    }
    for sql in QUERY_PRAGMAS {
        let _mode: String = conn.query_row(sql, [], |row| row.get(0)).map_err(map_err)?;
    }
    if let Err(err) = conn.query_row(MMAP_PRAGMA, [], |row| row.get::<_, i64>(0)) {
        tracing::warn!(error = %err, "mmap_size 设置失败，已降级（不影响正确性）");
    }
    Ok(())
}

/// 当前 journal 模式（测试与诊断用）。
pub fn journal_mode(conn: &Connection) -> Result<String, AppError> {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(map_err)
}

/// 外键开关是否生效（级联删除依赖它）。
pub fn foreign_keys_enabled(conn: &Connection) -> Result<bool, AppError> {
    let value: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(map_err)?;
    Ok(value == 1)
}

/// 编译选项（供 TR-03 验证 bundled SQLite 是否含 FTS5）。
pub fn compile_options(conn: &Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare("PRAGMA compile_options").map_err(map_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(map_err)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_err)?);
    }
    Ok(out)
}

fn map_err(err: rusqlite::Error) -> AppError {
    AppError::DbError(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_conn() -> Connection {
        Connection::open_in_memory().expect("内存库应可打开")
    }

    #[test]
    fn pragmas_apply_and_take_effect() {
        let conn = memory_conn();
        apply(&conn).expect("PRAGMA 应用应成功");
        assert!(foreign_keys_enabled(&conn).expect("查询外键开关应成功"));
        // 内存库无法使用 WAL（SQLite 对 :memory: 会回退），因此只断言查询可用
        assert!(!journal_mode(&conn)
            .expect("查询 journal_mode 应成功")
            .is_empty());
    }

    #[test]
    fn apply_is_idempotent() {
        let conn = memory_conn();
        apply(&conn).expect("第一次应用应成功");
        apply(&conn).expect("重复应用应成功");
    }

    #[test]
    fn fts5_availability_is_reported() {
        // TR-03：记录 bundled SQLite 的编译选项，PR-2 建 FTS5 表时据此判定
        let conn = memory_conn();
        let options = compile_options(&conn).expect("查询编译选项应成功");
        let has_fts5 = options.iter().any(|o| o.contains("ENABLE_FTS5"));
        assert!(!options.is_empty(), "编译选项不应为空");
        // TR-03：FTS5 是 M3 全文搜索的前提，bundled SQLite 必须启用它。
        // 若此断言失败，需在 Cargo.toml 追加 FTS5 相关 feature 或改用完整 bundled 构建。
        assert!(
            has_fts5,
            "bundled SQLite 未启用 FTS5，TR-03 需重新评估：{options:?}"
        );
    }
}
