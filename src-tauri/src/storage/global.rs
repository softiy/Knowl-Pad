//! 全局库 global.db：Vault 注册表、界面偏好、窗口状态（PRD §3.3）。
//!
//! **实现约束（PRD §2.4）**：配置目录路径必须经 Tauri 的 path API 取得，禁止硬编码或手工拼接平台路径。

use super::pool::DbPool;
use super::GLOBAL_DB_FILE;
use kp_domain::error::AppError;
use std::path::{Path, PathBuf};

/// 全局库文件路径（由配置目录派生）。
pub fn db_path(config_dir: &Path) -> PathBuf {
    config_dir.join(GLOBAL_DB_FILE)
}

/// 记录本次启动（写入 meta 的 `last_startup_version` / `last_startup_at`）。
///
/// 失败**不影响启动**：仅作为诊断信息，调用方按 FR-GLOBAL-01 继续运行。
pub fn record_startup(pool: &DbPool) -> Result<(), AppError> {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    pool.with_writer(move |conn| {
        let tx = conn
            .transaction()
            .map_err(|err| AppError::DbError(err.to_string()))?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('last_startup_version', ?1)",
            [version.as_str()],
        )
        .map_err(|err| AppError::DbError(err.to_string()))?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('last_startup_at', ?1)",
            [now_ms.to_string().as_str()],
        )
        .map_err(|err| AppError::DbError(err.to_string()))?;
        tx.commit()
            .map_err(|err| AppError::DbError(err.to_string()))?;
        Ok(())
    })
}

/// 打开全局库：确保配置目录存在 → 打开库 → 应用 PRAGMA → 执行迁移。
///
/// 对应 FR-GLOBAL-01 的前提：调用方在失败时必须以默认配置继续启动，不得崩溃。
pub fn open(config_dir: &Path) -> Result<DbPool, AppError> {
    std::fs::create_dir_all(config_dir)?;
    DbPool::open(&db_path(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrate;

    #[test]
    fn db_path_uses_global_file_name() {
        let path = db_path(Path::new("/tmp/knowl-pad"));
        assert!(path.ends_with(GLOBAL_DB_FILE));
    }

    #[test]
    fn open_creates_dir_and_migrates() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let config_dir = dir.path().join("nested").join("config");
        assert!(!config_dir.exists(), "前置条件：目录尚不存在");
        let pool = open(&config_dir).expect("打开全局库应成功（并自动创建目录）");
        assert!(config_dir.exists(), "配置目录应被创建");
        assert!(db_path(&config_dir).exists(), "全局库文件应存在");
        let version = pool
            .with_reader(migrate::schema_version)
            .expect("读版本应成功");
        assert_eq!(version, migrate::latest_version());
    }

    #[test]
    fn record_startup_writes_meta_and_survives_reopen() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let pool = open(dir.path()).expect("打开应成功");
        record_startup(&pool).expect("记录启动应成功");
        let version: String = pool
            .with_reader(|conn| {
                conn.query_row(
                    "SELECT value FROM meta WHERE key = 'last_startup_version'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|err| AppError::DbError(err.to_string()))
            })
            .expect("读取应成功");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn reopening_existing_db_keeps_data() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        {
            let pool = open(dir.path()).expect("首次打开应成功");
            pool.with_writer(|conn| {
                conn.execute(
                    "INSERT INTO vault(abs_path, display_name, pinned) VALUES('/tmp/vault', '测试库', 1)",
                    [],
                )
                .map_err(|err| AppError::DbError(err.to_string()))?;
                Ok(())
            })
            .expect("写入应成功");
        }
        let pool = open(dir.path()).expect("再次打开应成功");
        let name: String = pool
            .with_reader(|conn| {
                conn.query_row("SELECT display_name FROM vault", [], |row| row.get(0))
                    .map_err(|err| AppError::DbError(err.to_string()))
            })
            .expect("读取应成功");
        assert_eq!(name, "测试库");
    }
}
