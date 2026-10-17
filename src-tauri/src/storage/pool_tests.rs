//! pool.rs 的单元测试（CODE-11：测试位于独立文件）。

use super::*;
use crate::storage::migrate;
use std::sync::{Arc, Barrier};

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
#[test]
fn panicking_write_task_does_not_kill_the_pool() {
    // #8 回归：单个写任务 panic 不得让写线程退出（否则该库在整个进程内不可写）
    let (_dir, pool) = temp_pool();
    let failed = pool.with_writer(|_conn| -> Result<(), AppError> {
        panic!("模拟写任务 panic");
    });
    assert!(failed.is_err(), "panic 的任务应返回错误");

    pool.with_writer(|conn| {
        conn.execute(
            "INSERT INTO preference(key, value) VALUES('after.panic', 'ok')",
            [],
        )
        .map_err(|err| AppError::DbError(err.to_string()))?;
        Ok(())
    })
    .expect("写线程必须仍然可用（自愈）");
    let value: String = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT value FROM preference WHERE key = 'after.panic'",
                [],
                |row| row.get(0),
            )
            .map_err(|err| AppError::DbError(err.to_string()))
        })
        .expect("读取应成功");
    assert_eq!(value, "ok");
}

#[test]
fn nested_with_writer_is_rejected_instead_of_deadlocking() {
    // #8 回归：写线程内再调 with_writer 会自我等待 → 必须被拒绝而不是挂死
    let (_dir, pool) = temp_pool();
    let pool = std::sync::Arc::new(pool);
    let inner = std::sync::Arc::clone(&pool);
    let result = pool.with_writer(move |_conn| {
        inner.with_writer(|_| Ok(())) // 嵌套调用：应立即返回错误
    });
    assert!(result.is_err(), "嵌套调用必须被拒绝（否则会死锁）");
    // 池仍然可用
    pool.with_writer(|_conn| Ok(())).expect("池应仍然可用");
}

#[test]
fn dropping_pool_releases_database_file() {
    // #8 回归：Drop 必须 join 写线程，否则 Windows 上文件仍被占用、目录删不掉
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let db_path = dir.path().join("global.db");
    {
        let pool = DbPool::open(&db_path).expect("打开应成功");
        pool.with_writer(|conn| {
            conn.execute("INSERT INTO preference(key, value) VALUES('k', 'v')", [])
                .map_err(|err| AppError::DbError(err.to_string()))?;
            Ok(())
        })
        .expect("写入应成功");
    } // Drop 在此发生
    assert!(db_path.exists(), "库文件应存在");
    std::fs::remove_file(&db_path).expect("Drop 之后应可删除库文件（句柄已释放）");
}

/// **DEBT-40 回归（确定性不变量，不依赖「竞争是否显形」）**。
///
/// 库已是 WAL 时，journal_mode = WAL 退化为读头部（实测约 7.7 µs，不取排他锁），
/// 因此 4 条连接**并发**建立也不得有任何一条失败 —— 这条不变量在修复后是确定的：
/// 它不赌调度，只赌「WAL 库上的该 PRAGMA 不取排他锁」这一 SQLite 语义。
///
/// 修复前：建池前无人先把库切成 WAL（缺 prepare_wal），而这 4 条连接正是池会并发建立的那批
/// （r2d2 用 3 线程池派发建连任务）⇒ 首次建库时 1 条赢、2 条输
/// （ERROR r2d2: database is locked + 400 ms 退避，真机首启因此多花约 400 ms）。
/// 这里复用的 init_connection 与生产**逐字相同**，避免测试另抄一份副本后静默漂移。
#[test]
fn concurrent_connections_on_prepared_wal_db_all_succeed() {
    use r2d2::ManageConnection as _;

    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let path = dir.path().join("global.db");
    prepare_wal(&path).expect("单连接准备 WAL 应成功");

    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let manager = SqliteConnectionManager::file(&path).with_init(init_connection);
                // 先对齐再建连：把「并发」从概率性拉成必然（否则慢机器上可能串行化而掩盖竞争）。
                barrier.wait();
                manager.connect().map(|_| ()).map_err(|err| err.to_string())
            })
        })
        .collect();

    for (index, handle) in handles.into_iter().enumerate() {
        let outcome = handle.join().expect("建连线程不应 panic");
        assert!(
            outcome.is_ok(),
            "第 {index} 条并发连接失败：{outcome:?}（DEBT-40 回归：prepare_wal 之后不应再争 WAL 转换锁）"
        );
    }
}
