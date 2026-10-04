//! 设置与系统域命令（PRD §5.3.9）：偏好（全局）+ 界面状态（每 Vault）。
//!
//! - `preference_*`：全局库的 `preference` 表，跨 Vault 生效（如「显示隐藏文件」）；
//! - `vault_state_*`：全局库的 `vault_state` 表，按 Vault 隔离（如文件树展开状态、滚动位置）。
//!
//! 两边的值都以 **JSON** 传输与存储（`Record<string, unknown>` 契约）。

use crate::error_wrapper::KpError;
use crate::state::AppState;
use crate::storage::{global, vault_state};
use kp_domain::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysArgs {
    /// 缺省表示读取全部
    pub keys: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntriesArgs {
    pub entries: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuesResult {
    pub values: BTreeMap<String, serde_json::Value>,
}

fn parse_value(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.to_string()))
}

/// 读取偏好（全局）。
#[tauri::command]
pub async fn preference_get(
    state: tauri::State<'_, AppState>,
    args: KeysArgs,
) -> Result<ValuesResult, KpError> {
    let pool = state
        .global_db()
        .ok_or_else(|| KpError(AppError::db("访问偏好存储")))?;
    let mut values = BTreeMap::new();
    match args.keys {
        Some(keys) => {
            for key in keys {
                if let Some(raw) = global::read_preference(&pool, &key).map_err(KpError)? {
                    values.insert(key, parse_value(&raw));
                }
            }
        }
        None => {
            for (key, raw) in global::list_preferences(&pool).map_err(KpError)? {
                values.insert(key, parse_value(&raw));
            }
        }
    }
    Ok(ValuesResult { values })
}

/// 批量写入偏好（全局）。
#[tauri::command]
pub async fn preference_set(
    state: tauri::State<'_, AppState>,
    args: EntriesArgs,
) -> Result<(), KpError> {
    let pool = state
        .global_db()
        .ok_or_else(|| KpError(AppError::db("访问偏好存储")))?;
    for (key, value) in args.entries {
        global::set_preference(&pool, &key, &value.to_string()).map_err(KpError)?;
    }
    Ok(())
}

/// 读取当前 Vault 的界面状态。
#[tauri::command]
pub async fn vault_state_get(
    state: tauri::State<'_, AppState>,
    args: KeysArgs,
) -> Result<ValuesResult, KpError> {
    let (pool, vault_id) = open_vault(&state)?;
    let mut values = BTreeMap::new();
    match args.keys {
        Some(keys) => {
            for key in keys {
                if let Some(raw) =
                    vault_state::read_state(&pool, vault_id, &key).map_err(KpError)?
                {
                    values.insert(key, parse_value(&raw));
                }
            }
        }
        None => {
            for (key, raw) in vault_state::read_all(&pool, vault_id).map_err(KpError)? {
                values.insert(key, parse_value(&raw));
            }
        }
    }
    Ok(ValuesResult { values })
}

/// 批量写入当前 Vault 的界面状态。
#[tauri::command]
pub async fn vault_state_set(
    state: tauri::State<'_, AppState>,
    args: EntriesArgs,
) -> Result<(), KpError> {
    let (pool, vault_id) = open_vault(&state)?;
    for (key, value) in args.entries {
        vault_state::write_state(&pool, vault_id, &key, &value.to_string()).map_err(KpError)?;
    }
    Ok(())
}

fn open_vault(
    state: &tauri::State<'_, AppState>,
) -> Result<(std::sync::Arc<crate::storage::pool::DbPool>, i64), KpError> {
    let pool = state
        .global_db()
        .ok_or_else(|| KpError(AppError::db("访问全局注册表")))?;
    // 未打开 Vault 时按契约返回 E_VAULT_NOT_OPEN（界面状态按 Vault 隔离）
    let vault_id = state
        .current_vault_id()
        .ok_or(KpError(AppError::VaultNotOpen))?;
    Ok((pool, vault_id))
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
