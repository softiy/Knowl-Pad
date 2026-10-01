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
        let tx = conn.transaction().map_err(map_err)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('last_startup_version', ?1)",
            [version.as_str()],
        )
        .map_err(map_err)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('last_startup_at', ?1)",
            [now_ms.to_string().as_str()],
        )
        .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
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

/// Vault 注册表行（PRD §3.3 的 `vault` 表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRow {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened: Option<i64>,
    pub pinned: bool,
    pub trust_level: String,
}

/// 注册 Vault（按 `abs_path` 幂等）。
///
/// 冲突时**只更新 last_opened**，保留既有 `display_name`——否则用户重命名显示名后，
/// 每次打开都会被目录名覆盖（FR-VAULT-07）。返回 `vault_id`。
pub fn upsert_vault(pool: &DbPool, abs_path: &str, display_name: &str) -> Result<i64, AppError> {
    let path = abs_path.to_string();
    let name = display_name.to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO vault(abs_path, display_name, last_opened)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(abs_path) DO UPDATE SET last_opened = excluded.last_opened",
            rusqlite::params![path, name, now_ms()],
        )
        .map_err(map_err)?;
        conn.query_row(
            "SELECT id FROM vault WHERE abs_path = ?1",
            [path.as_str()],
            |row| row.get(0),
        )
        .map_err(map_err)
    })
}

/// 列出全部注册 Vault：置顶优先，其次按最近打开时间倒序（PRD §3.3 / FR-VAULT-07）。
pub fn list_vaults(pool: &DbPool) -> Result<Vec<VaultRow>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, abs_path, display_name, last_opened, pinned, trust_level
                 FROM vault
                 ORDER BY pinned DESC, COALESCE(last_opened, 0) DESC, id ASC",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(VaultRow {
                    id: row.get(0)?,
                    abs_path: row.get(1)?,
                    display_name: row.get(2)?,
                    last_opened: row.get(3)?,
                    pinned: row.get::<_, i64>(4)? != 0,
                    trust_level: row.get(5)?,
                })
            })
            .map_err(map_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(map_err)?);
        }
        Ok(out)
    })
}

/// 从注册表移除（**仅删注册记录，绝不触碰磁盘文件** —— AC-VAULT-04）。
/// 返回是否确实删除了记录。
pub fn remove_vault(pool: &DbPool, vault_id: i64) -> Result<bool, AppError> {
    pool.with_writer(move |conn| {
        let affected = conn
            .execute("DELETE FROM vault WHERE id = ?1", [vault_id])
            .map_err(map_err)?;
        Ok(affected > 0)
    })
}

/// 修改显示名（FR-VAULT-07）。返回是否命中记录。
pub fn rename_vault(pool: &DbPool, vault_id: i64, display_name: &str) -> Result<bool, AppError> {
    let name = display_name.to_string();
    pool.with_writer(move |conn| {
        let affected = conn
            .execute(
                "UPDATE vault SET display_name = ?1 WHERE id = ?2",
                rusqlite::params![name, vault_id],
            )
            .map_err(map_err)?;
        Ok(affected > 0)
    })
}

/// 重新定位：更新注册路径（路径失效后由用户重新选择，FR-VAULT-08 / AC-VAULT-02）。
pub fn relocate_vault(pool: &DbPool, vault_id: i64, new_abs_path: &str) -> Result<(), AppError> {
    let path = new_abs_path.to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "UPDATE vault SET abs_path = ?1 WHERE id = ?2",
            rusqlite::params![path, vault_id],
        )
        .map_err(map_err)?;
        Ok(())
    })
}

/// 当前毫秒时间戳（Unix epoch）。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn map_err(err: rusqlite::Error) -> AppError {
    // ERR-02：SQL 原文等技术细节只进日志，用户可见 message 走中文可操作文案
    tracing::warn!(error = %err, "知识库注册表操作失败");
    AppError::db("访问知识库注册表")
}

/// preference 键：上次打开的 Vault id（FR-VAULT-06）。
pub const PREF_LAST_VAULT: &str = "vault.last_opened";

/// preference 键：是否在启动时恢复上次的 Vault（FR-VAULT-06「可在设置中关闭」）。
pub const PREF_RESTORE_LAST: &str = "vault.restore_last";

/// 记录「上次打开的 Vault」（用户主动关闭时由 clear_last_vault 清除）。
pub fn set_last_vault(pool: &DbPool, vault_id: i64) -> Result<(), AppError> {
    set_preference(pool, PREF_LAST_VAULT, &vault_id.to_string())
}

/// 清除「上次打开的 Vault」——用户显式关闭/移除后不应再自动恢复。
pub fn clear_last_vault(pool: &DbPool) -> Result<(), AppError> {
    pool.with_writer(|conn| {
        conn.execute("DELETE FROM preference WHERE key = ?1", [PREF_LAST_VAULT])
            .map_err(map_err)?;
        Ok(())
    })
}

/// 读取「上次打开的 Vault」对应的注册行（记录不存在或已失效时返回 None）。
pub fn last_vault(pool: &DbPool) -> Result<Option<VaultRow>, AppError> {
    let Some(raw) = read_preference(pool, PREF_LAST_VAULT)? else {
        return Ok(None);
    };
    let Ok(id) = raw.trim().parse::<i64>() else {
        tracing::warn!("上次 Vault 记录格式非法，已忽略");
        return Ok(None);
    };
    pool.with_reader(|conn| {
        match conn.query_row(
            "SELECT id, abs_path, display_name, last_opened, pinned, trust_level FROM vault WHERE id = ?1",
            [id],
            |row| {
                Ok(VaultRow {
                    id: row.get(0)?,
                    abs_path: row.get(1)?,
                    display_name: row.get(2)?,
                    last_opened: row.get(3)?,
                    pinned: row.get::<_, i64>(4)? != 0,
                    trust_level: row.get(5)?,
                })
            },
        ) {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(map_err(err)),
        }
    })
}

/// 是否允许启动时恢复上次 Vault（默认允许）。
pub fn restore_last_enabled(pool: &DbPool) -> Result<bool, AppError> {
    Ok(read_preference(pool, PREF_RESTORE_LAST)?
        .map(|raw| raw.trim() != "false" && raw.trim() != "0")
        .unwrap_or(true))
}

/// 写入界面偏好（PRD §3.3：preference.value 为 JSON 编码，此处按标量字符串存储）。
pub fn set_preference(pool: &DbPool, key: &str, value: &str) -> Result<(), AppError> {
    let key = key.to_string();
    let value = value.to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO preference(key, value) VALUES(?1, ?2)",
            rusqlite::params![key, value],
        )
        .map_err(map_err)?;
        Ok(())
    })
}

/// 读取界面偏好。
pub fn read_preference(pool: &DbPool, key: &str) -> Result<Option<String>, AppError> {
    let key = key.to_string();
    pool.with_reader(move |conn| {
        match conn.query_row(
            "SELECT value FROM preference WHERE key = ?1",
            [key.as_str()],
            |row| row.get::<_, String>(0),
        ) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(map_err(err)),
        }
    })
}

#[cfg(test)]
#[path = "global_registry_tests.rs"]
mod registry_tests;
#[cfg(test)]
#[path = "global_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "global_preference_tests.rs"]
mod preference_tests;
