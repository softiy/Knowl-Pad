//! 全局库的**恢复**行为（FR-GLOBAL-01 / DEBT-20）：损坏保留后重建、缺失按新库、正常零打扰。
//!
//! 这些用例就是 DEBT-20 要求的四条确定性判据（父任务裁定里的 1~4）。

use super::*;
// 恢复相关用例还要用到"正常库"那一侧的登记/读取函数与路径推导（都在 `global` 模块里）
use crate::storage::global::{db_path, list_vaults, read_preference, upsert_vault};

/// 造一个"打不开"的全局库文件（不是 SQLite 格式）。
fn write_corrupt(dir: &Path) -> PathBuf {
    let path = db_path(dir);
    std::fs::write(&path, b"not a sqlite database at all").expect("写坏文件应成功");
    path
}

fn corrupt_backups(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("读目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".corrupt-") || n.contains(".recovered-"))
        .collect()
}

/// ① 损坏的全局库 ⇒ 启动成功、旧文件被**重命名保留**、新库可用。
#[test]
fn corrupt_db_is_preserved_and_rebuilt() {
    let dir = tempfile::tempdir().expect("临时目录");
    let corrupt = write_corrupt(dir.path());

    let opened = open_with_recovery(dir.path()).expect("应能恢复并启动");
    assert!(opened.outcome.rebuilt, "必须标记为重建过");
    let preserved = opened.outcome.preserved.clone().expect("应保留原文件");
    // 恢复后**原路径存在的是新建的空库**（不是原文件）；两者必须是不同文件
    assert!(corrupt.exists(), "原路径应已由新库占位");
    assert_ne!(preserved, corrupt, "保留文件不能还是原路径");
    assert!(
        preserved.exists(),
        "保留文件必须真的在磁盘上（绝不删用户数据）"
    );
    assert_eq!(
        std::fs::read(&preserved).expect("读保留文件"),
        b"not a sqlite database at all",
        "保留的必须是**原字节**，未被改写"
    );
    assert_eq!(
        corrupt_backups(dir.path()).len(),
        1,
        "恰好一个保留文件，不重复留档"
    );

    // 新库可用：能注册 Vault、能读偏好（证明迁移在新库上跑完了）
    let id = upsert_vault(&opened.pool, "/vault/a", "A").expect("新库应可写");
    assert!(id > 0);
    assert_eq!(read_preference(&opened.pool, "nope").expect("应可读"), None);

    // 恢复信息（IPC 口径）只带文件名，且给出原因
    let recovery = opened.outcome.recovery().expect("重建后应有恢复信息");
    assert!(recovery.preserved_file.is_some());
    assert!(
        !recovery
            .preserved_file
            .expect("文件名")
            .contains(std::path::MAIN_SEPARATOR),
        "只给文件名，不带目录"
    );
    assert!(!recovery.reason.is_empty(), "原因不得为空（便于诊断）");
}

/// ① 的补充（直接测保留逻辑，确定性）：主库与 `-wal`/`-shm` **一起**挪到留档名旁边。
#[test]
fn preserve_corrupt_moves_main_and_sidecars() {
    let dir = tempfile::tempdir().expect("临时目录");
    let corrupt = write_corrupt(dir.path());
    let wal = with_suffix(&corrupt, "-wal");
    let shm = with_suffix(&corrupt, "-shm");
    std::fs::write(&wal, b"stale wal").expect("写侧车");
    std::fs::write(&shm, b"stale shm").expect("写侧车");

    let outcome = preserve_corrupt(&corrupt, 1_761_000_000);
    assert!(
        outcome.problem.is_none(),
        "正常情况下不应有问题：{outcome:?}"
    );
    let target = outcome.preserved.expect("应保留原文件");
    assert_eq!(
        target.file_name().expect("文件名").to_string_lossy(),
        format!("{GLOBAL_DB_FILE}.corrupt-1761000000")
    );
    // 主库原字节被保留（绝不删用户数据），侧车跟着主库一起走
    assert_eq!(
        std::fs::read(&target).expect("读保留文件"),
        b"not a sqlite database at all"
    );
    assert!(with_suffix(&target, "-wal").exists());
    assert!(with_suffix(&target, "-shm").exists());
    assert!(
        !corrupt.exists() && !wal.exists() && !shm.exists(),
        "原路径已让开"
    );
}

/// ① 的补充（端到端）：失败的重建流程结束后，**原路径不得留下任何陈旧侧车**。
#[test]
fn corrupt_db_sidecars_do_not_survive_recovery() {
    let dir = tempfile::tempdir().expect("临时目录");
    let corrupt = write_corrupt(dir.path());
    let wal = with_suffix(&corrupt, "-wal");
    let shm = with_suffix(&corrupt, "-shm");
    std::fs::write(&wal, b"stale wal").expect("写侧车");
    std::fs::write(&shm, b"stale shm").expect("写侧车");

    let opened = open_with_recovery(dir.path()).expect("应能恢复并启动");
    assert!(opened.outcome.rebuilt);
    // 新库自己也会建 -wal/-shm，所以判据是"**旧内容**没有残留"（陈旧 WAL 会污染新库）
    assert_ne!(
        std::fs::read(&wal).unwrap_or_default(),
        b"stale wal".to_vec(),
        "陈旧 WAL 内容不得残留"
    );
    assert_ne!(
        std::fs::read(&shm).unwrap_or_default(),
        b"stale shm".to_vec(),
        "陈旧 SHM 内容不得残留"
    );
    // 新库可用，且留档文件名可读（只给文件名）
    assert!(upsert_vault(&opened.pool, "/vault/b", "B").is_ok());
    assert!(opened
        .outcome
        .preserved
        .clone()
        .expect("应保留原文件")
        .exists());
}

/// ② 缺失的全局库 ⇒ 与既有行为一致（新建，不算重建、不发提示）。
#[test]
fn missing_db_is_created_silently() {
    let dir = tempfile::tempdir().expect("临时目录");
    let opened = open_with_recovery(dir.path()).expect("应能新建");
    assert!(!opened.outcome.rebuilt, "缺失 ≠ 损坏，不得报「已重建」");
    assert!(opened.outcome.recovery().is_none(), "不得产生任何提示信息");
    assert!(
        corrupt_backups(dir.path()).is_empty(),
        "不得产生 .corrupt/.recovered 文件"
    );
    assert!(db_path(dir.path()).exists());
}

/// ③ 正常的全局库 ⇒ 行为完全不变：不重命名、不发提示、数据原样。
#[test]
fn healthy_db_is_untouched() {
    let dir = tempfile::tempdir().expect("临时目录");
    let first = open_with_recovery(dir.path()).expect("首次打开");
    let id = upsert_vault(&first.pool, "/vault/keep", "Keep").expect("写一条");
    assert!(!first.outcome.rebuilt);
    drop(first);

    let second = open_with_recovery(dir.path()).expect("再次打开");
    assert!(!second.outcome.rebuilt, "正常库不得被判为重建");
    assert!(second.outcome.recovery().is_none());
    assert!(corrupt_backups(dir.path()).is_empty(), "正常路径不得留档");
    let rows = list_vaults(&second.pool).expect("查询");
    let row = rows.iter().find(|r| r.id == id).expect("数据必须还在");
    assert_eq!(row.display_name, "Keep");
}

/// ④ 提示信息**恰好产生一次**：重建那一次有、紧随其后的正常打开没有。
#[test]
fn recovery_is_reported_exactly_once() {
    let dir = tempfile::tempdir().expect("临时目录");
    write_corrupt(dir.path());
    let first = open_with_recovery(dir.path()).expect("恢复并启动");
    assert!(
        first.outcome.recovery().is_some(),
        "第一次（重建）必须有提示信息"
    );
    drop(first);
    let second = open_with_recovery(dir.path()).expect("第二次正常打开");
    assert!(
        second.outcome.recovery().is_none(),
        "再次启动不得重复提示（正常启动零噪声）"
    );
}
