//! 每 Vault 的 UI 状态（PRD §3.3 的 `vault_state` 表；FR-FILE-01、FR-EDITOR-37、AC-FILE-08）。
//!
//! 与 `preference` 的分工：**偏好**是全局的（跨 Vault，如「显示隐藏文件」）；**vault_state** 是
//! 每个 Vault 各自的界面状态（如文件树展开了哪些目录、上次打开的标签页、滚动位置）。
//!
//! 值统一以 **JSON 文本**存储（读写两端由命令层负责序列化），键名约定见各调用点。

use super::pool::DbPool;
use kp_domain::error::AppError;

/// 单个状态值的长度上限：超过则**拒绝写入并告警**（避免异常路径把全局库撑爆）。
pub const MAX_STATE_BYTES: usize = 512 * 1024;

/// 读取单个键（不存在返回 None）。
pub fn read_state(pool: &DbPool, vault_id: i64, key: &str) -> Result<Option<String>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare_cached("SELECT value FROM vault_state WHERE vault_id = ?1 AND key = ?2")
            .map_err(map_err)?;
        let mut rows = stmt
            .query(rusqlite::params![vault_id, key])
            .map_err(map_err)?;
        match rows.next().map_err(map_err)? {
            Some(row) => Ok(Some(row.get::<_, String>(0).map_err(map_err)?)),
            None => Ok(None),
        }
    })
}

/// 读取全部键（无则返回空表）。
pub fn read_all(pool: &DbPool, vault_id: i64) -> Result<Vec<(String, String)>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare_cached("SELECT key, value FROM vault_state WHERE vault_id = ?1 ORDER BY key")
            .map_err(map_err)?;
        let rows = stmt
            .query_map(rusqlite::params![vault_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(map_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(map_err)?);
        }
        Ok(out)
    })
}

/// 写入（UPSERT）单个键。
pub fn write_state(pool: &DbPool, vault_id: i64, key: &str, value: &str) -> Result<(), AppError> {
    if value.len() > MAX_STATE_BYTES {
        tracing::warn!(key, bytes = value.len(), "界面状态过大，已跳过写入");
        return Err(AppError::IoFailure(
            "界面状态过大，已跳过保存（不影响笔记数据）".to_string(),
        ));
    }
    let key_owned = key.to_string();
    let value_owned = value.to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO vault_state(vault_id, key, value) VALUES(?1, ?2, ?3) \
             ON CONFLICT(vault_id, key) DO UPDATE SET value = excluded.value",
            rusqlite::params![vault_id, key_owned, value_owned],
        )
        .map_err(map_err)?;
        Ok(())
    })
}

fn map_err(err: rusqlite::Error) -> AppError {
    tracing::warn!(error = %err, "界面状态读写失败");
    AppError::db("读写界面状态")
}

#[cfg(test)]
#[path = "vault_state_tests.rs"]
mod tests;
