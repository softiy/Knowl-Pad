//! 索引库「丢弃重建」的**文件级机制**（技术方案 §4.4；PRD MIG-01 / FR-SIG-01/02/03、NFR-REL-07）。
//!
//! §4.4 规定的六步：
//! 1. 新建独立库文件 `index.db.rebuild`（**全程不触碰正在服务的 `index.db`**）
//! 2. 在 rebuild 库中写入 `meta.rebuild_in_progress`
//! 3. 触发全量索引，全部写入 `index.db.rebuild`
//! 4. 完成后清除标记并落盘
//! 5. 原子切换：rename(index.db → index.db.old) → rename(index.db.rebuild → index.db) → 删除 `.old` 及其 `-wal`/`-shm`
//! 6. 启动时若发现残留的 `index.db.rebuild`（上次中断）：丢弃它，按当前 `index.db` 状态重新开始
//!
//! **本次落地范围**：步骤 1/2/4/5/6 的文件级机制（M1 的"重建"= 建空 schema + 写签名，索引内容为空）。
//! **明确顺延 M3**：`kp://index/rebuild-required` 事件本体、重建进度上报，以及"重建期间读池只读旧库"的编排
//! （需要索引引擎与 UI 消费方）。本模块已把结果作为 [`RebuildOutcome`] 返回，接口就位。

use super::global::now_ms;
use super::index::{
    clear_meta, create_schema, read_meta, set_meta, stored_signature, write_meta, IndexSignature,
    META_REBUILD_IN_PROGRESS,
};
use super::pragma;
use kp_domain::error::AppError;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// 重建目标文件名（与 `index.db` 同目录）。
pub const REBUILD_DB_NAME: &str = "index.db.rebuild";

/// 切换前的旧库名（切换成功后连同 `-wal`/`-shm` 一并删除）。
pub const OLD_DB_NAME: &str = "index.db.old";

/// 索引库就绪的结果——M3 据此发出 `kp://index/rebuild-required` 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildOutcome {
    /// 首次创建（此前不存在索引库）
    Fresh,
    /// 已丢弃重建；`reason` 为人类可读原因（可直接用于事件与日志）
    Rebuilt { reason: String },
    /// 签名一致，沿用既有索引
    Unchanged,
}

/// 就绪索引库：必要时在独立文件中重建并**原子切换**（§4.4）。
pub fn ensure_index_db(
    db_path: &Path,
    expected: &IndexSignature,
) -> Result<RebuildOutcome, AppError> {
    let dir = db_path
        .parent()
        .ok_or_else(|| AppError::IoFailure("索引库路径无父目录".into()))?;
    std::fs::create_dir_all(dir)?;

    // 步骤 6：上次中断留下的重建产物一律丢弃，再按当前 index.db 状态重新判定
    if remove_db_files(&dir.join(REBUILD_DB_NAME))? {
        tracing::warn!("发现上次未完成的索引重建产物，已丢弃（FR-SIG-03 / NFR-REL-07）");
    }

    let reason = if db_path.exists() {
        match inspect(db_path, expected)? {
            Inspect::Matches => return Ok(RebuildOutcome::Unchanged),
            Inspect::Stale(diff) => Some(diff),
            Inspect::Untrusted => Some("索引库缺少签名记录（按不可信处理）".to_string()),
            Inspect::Interrupted => Some("上次重建被中断（残留标记）".to_string()),
        }
    } else {
        None
    };

    build_rebuild_file(dir, expected)?; // 步骤 1-4
    switch_in(db_path)?; // 步骤 5

    Ok(match reason {
        None => RebuildOutcome::Fresh,
        Some(reason) => RebuildOutcome::Rebuilt { reason },
    })
}

/// 既有 `index.db` 的可信度判定。
enum Inspect {
    Matches,
    Stale(String),
    Untrusted,
    Interrupted,
}

fn inspect(db_path: &Path, expected: &IndexSignature) -> Result<Inspect, AppError> {
    // 只读短连接：读取失败（例如文件损坏）一律按「不可信」处理——索引是可丢弃的派生数据，
    // 重建永远优于让用户打不开 Vault。
    let conn = match Connection::open(db_path) {
        Ok(conn) => conn,
        Err(err) => {
            tracing::warn!(error = %err, "索引库无法打开，按不可信处理");
            return Ok(Inspect::Untrusted);
        }
    };
    if let Ok(Some(_)) = read_meta(&conn, META_REBUILD_IN_PROGRESS) {
        return Ok(Inspect::Interrupted);
    }
    match stored_signature(&conn) {
        Ok(Some(stored)) if stored.digest == expected.digest => Ok(Inspect::Matches),
        Ok(Some(stored)) => Ok(Inspect::Stale(expected.diff_reason(&stored))),
        // meta 表存在但缺签名（或读取失败）：缺失 ≠ 一致（FR-SIG-01）
        Ok(None) => Ok(Inspect::Untrusted),
        Err(err) => {
            tracing::warn!(error = %err, "索引签名读取失败，按不可信处理");
            Ok(Inspect::Untrusted)
        }
    }
}

/// 步骤 1-4：在独立文件中建库（schema + meta + 签名），完成后清除重建标记并落盘。
fn build_rebuild_file(dir: &Path, expected: &IndexSignature) -> Result<(), AppError> {
    let tmp = dir.join(REBUILD_DB_NAME);
    remove_db_files(&tmp)?;

    let mut conn = Connection::open(&tmp).map_err(map_err)?;
    pragma::apply(&conn)?;

    let tx = conn.transaction().map_err(map_err)?;
    create_schema(&tx)?;
    // 步骤 2：写入重建标记。真正"上次中断"的证据是**残留文件本身**（步骤 6），
    // 标记用于诊断，并在切换前清除（否则新 index.db 会被误判为重建中）。
    set_meta(&tx, META_REBUILD_IN_PROGRESS, &now_ms().to_string())?;
    write_meta(&tx, expected)?;
    tx.commit().map_err(map_err)?;

    // 步骤 4：清除标记并确保数据落盘
    clear_meta(&conn, META_REBUILD_IN_PROGRESS)?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    drop(conn);
    tracing::debug!(file = REBUILD_DB_NAME, "重建库已就绪");
    Ok(())
}

/// 步骤 5：两次 rename 原子切换；失败时把旧库改回，避免"切换失败且旧库消失"。
fn switch_in(db_path: &Path) -> Result<(), AppError> {
    let dir = db_path
        .parent()
        .ok_or_else(|| AppError::IoFailure("索引库路径无父目录".into()))?;
    let rebuild = dir.join(REBUILD_DB_NAME);
    let old = dir.join(OLD_DB_NAME);

    remove_db_files(&old)?; // 清理上次可能残留的 .old
    let had_old = db_path.exists();
    if had_old {
        std::fs::rename(db_path, &old)?; // rename 1
    }
    if let Err(err) = std::fs::rename(&rebuild, db_path) {
        if had_old {
            let _ = std::fs::rename(&old, db_path); // 回滚：恢复旧库
        }
        tracing::warn!(error = %err, "索引库切换失败，已回滚");
        return Err(AppError::from(err));
    }
    if had_old {
        remove_db_files(&old)?; // 删除旧库及其 -wal/-shm
    }
    tracing::info!("索引库已原子切换（§4.4 步骤 5）");
    Ok(())
}

/// 删除库文件及其 `-wal`/`-shm` 附属文件；返回主文件此前是否存在。
fn remove_db_files(path: &Path) -> Result<bool, AppError> {
    let existed = path.exists();
    let mut targets: Vec<PathBuf> = vec![path.to_path_buf()];
    for suffix in ["-wal", "-shm"] {
        targets.push(PathBuf::from(format!("{}{suffix}", path.to_string_lossy())));
    }
    for target in targets {
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
    }
    Ok(existed)
}

fn map_err(err: rusqlite::Error) -> AppError {
    tracing::warn!(error = %err, "索引库重建失败");
    AppError::db("重建索引库")
}
