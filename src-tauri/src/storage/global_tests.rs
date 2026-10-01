//! global.rs 的 tests 测试（CODE-11：测试位于独立文件）。

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
            .map_err(map_err)
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
                .map_err(map_err)?;
                Ok(())
            })
            .expect("写入应成功");
    }
    let pool = open(dir.path()).expect("再次打开应成功");
    let name: String = pool
        .with_reader(|conn| {
            conn.query_row("SELECT display_name FROM vault", [], |row| row.get(0))
                .map_err(map_err)
        })
        .expect("读取应成功");
    assert_eq!(name, "测试库");
}
