//! 连接管理（技术方案 §4.1）：**单写连接 + 读连接池**。
//!
//! SQLite 是「单写多读」模型；本项目读写比极不均衡。全部写操作经 channel 送入唯一写线程串行执行，
//! 彻底规避 SQLITE_BUSY 与写锁竞争（NFR-REL-09）；读操作走 r2d2 连接池并发执行。

use super::pragma;
use kp_domain::error::AppError;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender};

/// 写线程任务：一个持有写连接可变引用的闭包。
type Job = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

/// 读连接池默认大小（技术方案 §4.1：默认 4）。
const READER_POOL_SIZE: u32 = 4;

/// 数据库句柄：写连接（专用线程独占）+ 读连接池。
pub struct DbPool {
    path: PathBuf,
    writer: SyncSender<Job>,
    readers: r2d2::Pool<SqliteConnectionManager>,
}

impl DbPool {
    /// 打开（或创建）全局库：应用 PRAGMA → 执行**版本化迁移** → 启动写线程。
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::open_with(path, |conn| super::migrate::migrate(conn).map(|_| ()))
    }

    /// 打开（或创建）数据库：应用 PRAGMA → 执行调用方给定的 schema 引导逻辑 → 启动写线程。
    ///
    /// 引导逻辑在写线程内、**在 open 返回之前**完成；失败则 open 直接失败（不留下半初始化状态）。
    /// 全局库用版本化迁移（§4.5），索引库用**丢弃重建**（§4.4）——两者策略不同，故由此参数注入。
    pub fn open_with<B>(path: &Path, bootstrap: B) -> Result<Self, AppError>
    where
        B: FnOnce(&mut Connection) -> Result<(), AppError> + Send + 'static,
    {
        let manager = SqliteConnectionManager::file(path).with_init(|conn| {
            // 池中每条连接创建时应用一次 PRAGMA（幂等）
            pragma::apply(conn).map_err(|err| {
                rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(1), Some(err.to_string()))
            })
        });
        let readers = r2d2::Pool::builder()
            .max_size(READER_POOL_SIZE)
            .build(manager)
            .map_err(|err| AppError::DbError(format!("读连接池创建失败：{err}")))?;

        let (writer_tx, writer_rx) = sync_channel::<Job>(64);
        let (ready_tx, ready_rx) = sync_channel::<Result<(), AppError>>(1);
        let path_owned = path.to_path_buf();

        std::thread::Builder::new()
            .name("kp-db-writer".into())
            .spawn(move || {
                let mut conn = match Connection::open(&path_owned) {
                    Ok(conn) => conn,
                    Err(err) => {
                        let _ = ready_tx.send(Err(AppError::DbError(err.to_string())));
                        return;
                    }
                };
                if let Err(err) = pragma::apply(&conn) {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
                if let Err(err) = bootstrap(&mut conn) {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
                let _ = ready_tx.send(Ok(()));
                // 写连接由本线程独占：任务逐个串行执行，无需再加锁
                while let Ok(job) = writer_rx.recv() {
                    job(&mut conn);
                }
            })
            .map_err(|err| AppError::DbError(format!("写线程创建失败：{err}")))?;

        ready_rx
            .recv()
            .map_err(|_| AppError::DbError("写线程未就绪即退出".into()))??;

        Ok(Self {
            path: path.to_path_buf(),
            writer: writer_tx,
            readers,
        })
    }

    /// 数据库文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 在唯一写连接上执行闭包（全部写操作由此串行化）。
    pub fn with_writer<T, F>(&self, f: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, AppError> + Send + 'static,
    {
        let (tx, rx) = sync_channel::<Result<T, AppError>>(1);
        self.writer
            .send(Box::new(move |conn| {
                let _ = tx.send(f(conn));
            }))
            .map_err(|_| AppError::DbError("写线程已退出".into()))?;
        rx.recv()
            .map_err(|_| AppError::DbError("写线程未返回结果".into()))?
    }

    /// 刷盘：把 WAL 内容写回主库并截断 WAL 文件（关闭 Vault 前调用，FR-VAULT-05）。
    pub fn checkpoint(&self) -> Result<(), AppError> {
        self.with_writer(|conn| {
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(()),
                    other => Err(AppError::DbError(other.to_string())),
                })
        })
    }

    /// 从读连接池取一条连接执行闭包。
    pub fn with_reader<T, F>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&Connection) -> Result<T, AppError>,
    {
        let conn = self
            .readers
            .get()
            .map_err(|err| AppError::DbError(format!("取读连接失败：{err}")))?;
        f(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrate;

    fn temp_pool() -> (tempfile::TempDir, DbPool) {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let pool = DbPool::open(&dir.path().join("global.db")).expect("打开库应成功");
        (dir, pool)
    }

    #[test]
    fn open_runs_migration_and_creates_file() {
        let (dir, pool) = temp_pool();
        assert!(pool.path().exists(), "库文件应被创建");
        let version = pool
            .with_reader(migrate::schema_version)
            .expect("读版本应成功");
        assert_eq!(version, migrate::latest_version());
        drop(pool);
        drop(dir);
    }

    #[test]
    fn writer_and_reader_roundtrip() {
        let (_dir, pool) = temp_pool();
        pool.with_writer(|conn| {
            // 值用参数绑定，避免在 SQL 字符串里手写引号（JSON 值本身含双引号）
            conn.execute(
                "INSERT INTO preference(key, value) VALUES('theme.mode', ?1)",
                ["\"light\""],
            )
            .map_err(|err| AppError::DbError(err.to_string()))?;
            Ok(())
        })
        .expect("写入应成功");
        let value: String = pool
            .with_reader(|conn| {
                conn.query_row(
                    "SELECT value FROM preference WHERE key = 'theme.mode'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|err| AppError::DbError(err.to_string()))
            })
            .expect("读取应成功");
        assert_eq!(value, "\"light\"");
    }

    #[test]
    fn writer_serializes_concurrent_writes() {
        let (_dir, pool) = temp_pool();
        pool.with_writer(|conn| {
            conn.execute_batch("CREATE TABLE IF NOT EXISTS counter(n INTEGER NOT NULL)")
                .map_err(|err| AppError::DbError(err.to_string()))?;
            conn.execute("INSERT INTO counter(n) VALUES(0)", [])
                .map_err(|err| AppError::DbError(err.to_string()))?;
            Ok(())
        })
        .expect("初始化应成功");

        let pool = std::sync::Arc::new(pool);
        let mut handles = Vec::new();
        for _ in 0..8 {
            let pool = std::sync::Arc::clone(&pool);
            handles.push(std::thread::spawn(move || {
                for _ in 0..20 {
                    pool.with_writer(|conn| {
                        conn.execute("UPDATE counter SET n = n + 1", [])
                            .map_err(|err| AppError::DbError(err.to_string()))?;
                        Ok(())
                    })
                    .expect("并发写入不应失败");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("线程不应 panic");
        }
        let total: i64 = pool
            .with_reader(|conn| {
                conn.query_row("SELECT n FROM counter", [], |row| row.get(0))
                    .map_err(|err| AppError::DbError(err.to_string()))
            })
            .expect("统计应成功");
        assert_eq!(total, 160, "8 线程 × 20 次写必须全部生效（无丢失更新）");
    }

    #[test]
    fn concurrent_readers_are_allowed() {
        let (_dir, pool) = temp_pool();
        let pool = std::sync::Arc::new(pool);
        let mut handles = Vec::new();
        for _ in 0..6 {
            let pool = std::sync::Arc::clone(&pool);
            handles.push(std::thread::spawn(move || {
                let mode = pool
                    .with_reader(pragma::journal_mode)
                    .expect("并发读应成功");
                assert!(!mode.is_empty());
            }));
        }
        for handle in handles {
            handle.join().expect("线程不应 panic");
        }
    }
}
