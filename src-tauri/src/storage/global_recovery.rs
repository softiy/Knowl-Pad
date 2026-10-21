//! 全局库的**启动恢复**（FR-GLOBAL-01 / DEBT-20）：损坏时保留原文件后重建，绝不删用户数据。
//!
//! 单独成文件的两个原因：① `global.rs` 已贴近 CODE-11 的硬性上限（拆出后两文件都在预警线内）；
//! ② 恢复逻辑与"正常打开"是两类关注点 —— 前者是**启动编排**（何时保留、何时降级、如何告知一次），
//! 后者是存储层的常规打开。

use super::pool::DbPool;
use super::GLOBAL_DB_FILE;
use crate::storage::global::{db_path, open};
use kp_domain::error::AppError;
use std::path::{Path, PathBuf};

/// 全局库本次启动的**恢复结果**（FR-GLOBAL-01 / DEBT-20）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalOpenOutcome {
    /// 本次是否因"全局库不可用"而重建过（正常启动恒 `false`）。
    pub rebuilt: bool,
    /// 原库被**重命名保留**到的路径（连同 `-wal`/`-shm` 一起挪走）；未保留时为 `None`。
    pub preserved: Option<PathBuf>,
    /// 不可用的原因摘要（中文可读；**不含**用户数据）——即使重建成功也保留，便于诊断。
    pub reason: Option<String>,
}

/// 恢复信息（IPC/日志用）：只带**文件名**，不把配置目录的绝对路径暴露给界面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRecovery {
    /// 原库被保留成的文件名（如 `global.db.corrupt-1761000000`）。
    pub preserved_file: Option<String>,
    /// 重建原因摘要。
    pub reason: String,
}

impl GlobalOpenOutcome {
    /// 需要告诉用户的恢复信息；没有重建时返回 `None`（正常启动不得出现任何提示）。
    pub fn recovery(&self) -> Option<GlobalRecovery> {
        if !self.rebuilt {
            return None;
        }
        Some(GlobalRecovery {
            preserved_file: self
                .preserved
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string()),
            reason: self.reason.clone().unwrap_or_default(),
        })
    }
}

/// 打开结果：库句柄 + 恢复结果（两者一起返回，调用方才能"启动不中断 + 只提示一次"）。
pub struct GlobalDb {
    pub pool: DbPool,
    pub outcome: GlobalOpenOutcome,
}

/// **启动路径**的打开方式（FR-GLOBAL-01 / DEBT-20）：打不开就**保留原文件**再重建，绝不删用户数据。
///
/// 决策与取舍：
/// 1. 正常打开 ⇒ 原样返回（`rebuilt = false`，不产生任何文件、不发任何提示）；
/// 2. 打不开 ⇒ 把 `global.db` 重命名为 `global.db.corrupt-<unix 秒>`，`-wal`/`-shm` 一并挪走
///    （挪不动就删；两者都失败则视为"原路径不安全"），随后在原路径新建空库；
/// 3. **重命名也失败**（文件被占用/权限）⇒ 不动原文件，改用同目录的 `global.db.recovered-<秒>`
///    作为降级库 —— 应用仍能启动，恢复信息里带上失败原因；
/// 4. 连降级库都建不出来 ⇒ 返回 `Err`，调用方（`main.rs`）按既有 FR-GLOBAL-01 的兜底**继续启动**
///    （只是没有全局库：Vault 列表不可用，笔记数据在 Vault 内、不受影响）。
pub fn open_with_recovery(config_dir: &Path) -> Result<GlobalDb, AppError> {
    // 首次尝试复用严格打开（`open` 里已含"确保配置目录存在"），失败才进入保留逻辑
    match open(config_dir) {
        Ok(pool) => Ok(GlobalDb {
            pool,
            outcome: GlobalOpenOutcome::default(),
        }),
        Err(first) => {
            let reason = first.to_string();
            tracing::error!(error = %reason, "全局库不可用：将保留原文件并重建（FR-GLOBAL-01）");
            let stamp = unix_stamp();
            let db = db_path(config_dir);
            let preserve = preserve_corrupt(&db, stamp);
            // 侧车没处理干净时**不能**在原路径新建（陈旧 WAL 会污染新库）⇒ 走降级路径
            let target = match preserve.problem {
                None => db.clone(),
                Some(_) => config_dir.join(format!("{GLOBAL_DB_FILE}.recovered-{stamp}")),
            };
            let mut detail = reason;
            if let Some(problem) = preserve.problem {
                detail = format!("{detail}；{problem}");
            }
            let pool =
                DbPool::open(&target).map_err(|err| AppError::db_with("重建全局库", &err))?;
            Ok(GlobalDb {
                pool,
                outcome: GlobalOpenOutcome {
                    rebuilt: true,
                    preserved: preserve.preserved,
                    reason: Some(detail),
                },
            })
        }
    }
}

/// `preserve_corrupt` 的结果：保留到的路径 + 没解决的问题（侧车挪不动/删不掉等）。
#[derive(Debug)]
struct PreserveOutcome {
    preserved: Option<PathBuf>,
    problem: Option<String>,
}

/// unix 秒（**不引新依赖**：只需要一个稳定的时间戳片段，够区分多次启动即可）。
fn unix_stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 给路径加 SQLite 侧车后缀（`global.db` → `global.db-wal`）。
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

/// **保留**损坏的全局库（FR-GLOBAL-01 的"绝不删用户数据"）：主库改名、侧车一并处置。
///
/// 侧车（`-wal`/`-shm`）的顺序与兜底：先重命名；重命名失败就**删除**（它们只是崩溃恢复用的中间态，
/// 且原库已被保留）；两者都失败 ⇒ 记 `problem`，调用方改用降级路径（绝不能把它们留在新建库旁边）。
fn preserve_corrupt(db: &Path, stamp: u64) -> PreserveOutcome {
    let target = db.with_file_name(format!("{GLOBAL_DB_FILE}.corrupt-{stamp}"));
    if let Err(err) = std::fs::rename(db, &target) {
        return PreserveOutcome {
            preserved: None,
            problem: Some(format!("重命名全局库失败：{err}")),
        };
    }
    let mut problem: Option<String> = None;
    for suffix in ["-wal", "-shm"] {
        let side = with_suffix(db, suffix);
        if !side.exists() {
            continue;
        }
        if let Err(err) = std::fs::rename(&side, with_suffix(&target, suffix)) {
            match std::fs::remove_file(&side) {
                Ok(()) => tracing::warn!(error = %err, suffix, "侧车无法重命名，已删除"),
                Err(err2) => {
                    problem = Some(format!(
                        "侧车 {suffix} 无法移动（{err}）也无法删除（{err2}）"
                    ));
                    break;
                }
            }
        }
    }
    PreserveOutcome {
        preserved: Some(target),
        problem,
    }
}

#[cfg(test)]
#[path = "global_recovery_tests.rs"]
mod recovery_tests;
