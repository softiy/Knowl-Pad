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
use std::sync::Mutex;

/// 写线程任务：一个持有写连接可变引用的闭包。
type Job = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

thread_local! {
    /// 标记「当前线程是数据库写线程」——用于拒绝 with_writer 的嵌套调用（会死锁）。
    static ON_WRITER_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 读连接池默认大小（技术方案 §4.1：默认 4）。
const READER_POOL_SIZE: u32 = 4;

/// 数据库句柄：写连接（专用线程独占）+ 读连接池。
pub struct DbPool {
    path: PathBuf,
    writer: SyncSender<Job>,
    /// 写线程句柄：Drop 时用于关闭通道并 join，确保 SQLite 连接与文件句柄被释放
    writer_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    readers: r2d2::Pool<SqliteConnectionManager>,
}

impl DbPool {
    /// 打开（或创建）全局库：应用 PRAGMA → 执行**版本化迁移** → 启动写线程。
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let db_path = path.to_path_buf();
        Self::open_with(path, move |conn| {
            // DEBT-24①：**真正的升级**（已有库且版本低于目标）之前先把库备份出来。
            // 新建库（version 0）不需要备份；版本已是最新时也不产生多余文件。
            let from = super::migrate::schema_version(conn)?;
            if from > 0 && from < super::migrate::latest_version() {
                super::migrate::backup_before_migrate(conn, &db_path, from)?;
            }
            super::migrate::migrate(conn).map(|_| ())
        })
    }

    /// 打开（或创建）数据库：应用 PRAGMA → 执行调用方给定的 schema 引导逻辑 → 启动写线程。
    ///
    /// 引导逻辑在写线程内、**在 open 返回之前**完成；失败则 open 直接失败（不留下半初始化状态）。
    /// 全局库用版本化迁移（§4.5），索引库用**丢弃重建**（§4.4）——两者策略不同，故由此参数注入。
    pub fn open_with<B>(path: &Path, bootstrap: B) -> Result<Self, AppError>
    where
        B: FnOnce(&mut Connection) -> Result<(), AppError> + Send + 'static,
    {
        // DEBT-40：WAL 转换必须先由**单条**连接完成，再让池的并发连接建连（详见 prepare_wal）。
        prepare_wal(path)?;
        let manager = SqliteConnectionManager::file(path).with_init(init_connection);
        let readers = r2d2::Pool::builder()
            .max_size(READER_POOL_SIZE)
            .build(manager)
            .map_err(|err| {
                tracing::warn!(error = %err, "读连接池创建失败");
                AppError::db("初始化数据库读连接")
            })?;

        let (writer_tx, writer_rx) = sync_channel::<Job>(64);
        let (ready_tx, ready_rx) = sync_channel::<Result<(), AppError>>(1);
        let path_owned = path.to_path_buf();

        let writer_thread = std::thread::Builder::new()
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
                // 写连接由本线程独占：任务逐个串行执行，无需再加锁。
                // catch_unwind：单个任务 panic 不得让写线程退出——否则该库在整个进程生命周期内
                // 不可写（无自愈路径）。panic 会被转换为调用侧的 E_DB_ERROR。
                while let Ok(job) = writer_rx.recv() {
                    ON_WRITER_THREAD.with(|flag| flag.set(true));
                    let outcome =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut conn)));
                    ON_WRITER_THREAD.with(|flag| flag.set(false));
                    if outcome.is_err() {
                        tracing::error!(
                            "数据库写任务 panic，已捕获；该次操作未完成，连接仍可继续使用"
                        );
                    }
                }
            })
            .map_err(|err| {
                tracing::warn!(error = %err, "写线程创建失败");
                AppError::db("初始化数据库写线程")
            })?;

        ready_rx.recv().map_err(|_| {
            tracing::warn!("数据库写线程未就绪即退出");
            AppError::db("初始化数据库")
        })??;

        Ok(Self {
            path: path.to_path_buf(),
            writer: writer_tx,
            writer_thread: Mutex::new(Some(writer_thread)),
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
        // 防重入：在写线程内部再次调用 with_writer 会自我等待而永久死锁
        if ON_WRITER_THREAD.with(|flag| flag.get()) {
            tracing::error!("检测到写线程内嵌套调用 with_writer，已拒绝以避免死锁");
            return Err(AppError::db("执行嵌套数据库写入"));
        }
        let (tx, rx) = sync_channel::<Result<T, AppError>>(1);
        self.writer
            .send(Box::new(move |conn| {
                let _ = tx.send(f(conn));
            }))
            .map_err(|_| {
                tracing::warn!("数据库写线程已退出");
                AppError::db("写入数据库")
            })?;
        rx.recv().map_err(|_| {
            tracing::warn!("数据库写线程未返回结果");
            AppError::db("写入数据库")
        })?
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
        let conn = self.readers.get().map_err(|err| {
            tracing::warn!(error = %err, "取读连接失败");
            AppError::db("读取数据库")
        })?;
        f(&conn)
    }
}

/// 池内每条连接创建时调用的初始化钩子：应用一次 PRAGMA（幂等）。
///
/// 抽成具名函数有两个理由：① 让 pool_tests 的 DEBT-40 回归用例能用**与生产逐字相同**的钩子建连，
/// 而不是把 PRAGMA 序列与错误映射在测试里抄一份（副本会随生产漂移而静默失守）；
/// ② 把「错误怎么映射给 r2d2」这件事收在一处。
fn init_connection(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    pragma::apply(conn).map_err(|err| {
        // r2d2 只认 rusqlite 错误类型：把 AppError 的文案塞进 SqliteFailure，
        // 这样它落到日志里仍是 SQLite 原文（ERR-04），而不是被压成一句中文丢在池里。
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(1), Some(err.to_string()))
    })
}

/// 建池前的 WAL 准备：由**单条**连接完成 journal_mode 转换（DEBT-40）。
///
/// 为什么必须先做（2026-10-16 真机日志实证 + 探针复现）：journal_mode = WAL 在库还不是 WAL 时
/// 要**排他锁**完成转换，且该语句**不咨询 busy handler** —— busy_timeout = 5000 对它无效
/// （锁被占时实测约 1.7 ms 就返回 SQLITE_BUSY，而不是等 5 s）。而
/// r2d2::Pool::builder().build() 会阻塞到 min_idle（默认 = max_size = 4）条连接建立，
/// 建连任务由 r2d2 的 3 线程池并发派发 ⇒ **首次建库**时 3 条连接会同时抢这把锁：1 条赢、2 条失败。
/// r2d2 的默认错误处理器把失败打成 ERROR r2d2: database is locked（经 tracing-log 桥接后与项目
/// 日志同流），并以 400 ms 起步退避重试 ⇒ 首次启动白等约 400 ms（真机实测「启动 → 全局库已就绪」423 ms）。
/// 先由单连接转换后，池的并发连接看到的是**已是 WAL** 的库，该 PRAGMA 退化为读头部（实测约 7.7 µs）。
fn prepare_wal(path: &Path) -> Result<(), AppError> {
    let conn = Connection::open(path).map_err(|err| {
        tracing::warn!(error = %err, "读连接池建池前打开数据库失败");
        AppError::db("初始化数据库连接")
    })?;
    pragma::apply(&conn).map_err(|err| {
        tracing::warn!(error = %err, "读连接池建池前的 WAL 准备失败");
        AppError::db("初始化数据库连接")
    })
}

impl Drop for DbPool {
    /// 关闭写通道并 join 写线程，确保 SQLite 连接与文件句柄被**确定性**释放。
    ///
    /// 缺少此步时：写线程异步退出，Windows 上文件可能仍被占用——M3 删/换 index.db 会失败
    /// （本仓库在 M1 的测试中就实测到「池释放后目录删不掉」）。
    fn drop(&mut self) {
        // 用一个临时 sender 顶替，使原 sender 在此被丢弃 → 写线程 recv() 返回 Err 并退出
        let (placeholder, _) = sync_channel::<Job>(0);
        let real = std::mem::replace(&mut self.writer, placeholder);
        drop(real);
        if let Ok(mut guard) = self.writer_thread.lock() {
            if let Some(handle) = guard.take() {
                if handle.join().is_err() {
                    tracing::warn!("数据库写线程在退出时 panic");
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
