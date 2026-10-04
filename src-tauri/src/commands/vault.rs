//! Vault 相关命令（薄壳，RS-01）：实现见 vault_lifecycle.rs，数据结构见 vault_types.rs。

use super::vault_lifecycle::{activate_vault, close_current, current_vault, prepare_vault};
use super::vault_types::{
    IndexStatus, VaultCreateArgs, VaultIdArgs, VaultInfo, VaultOpenArgs, VaultPinArgs,
    VaultRelocateArgs, VaultRenameArgs, VaultSummary,
};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use std::path::PathBuf;

// ── 命令（薄壳，RS-01）────────────────────────────────────────────

/// 打开已有文件夹为 Vault（路径必须存在；否则 E_VAULT_PATH_INVALID 且不创建目录）。
#[tauri::command]
pub async fn vault_open(
    state: tauri::State<'_, AppState>,
    args: VaultOpenArgs,
) -> Result<VaultInfo, KpError> {
    let path = PathBuf::from(&args.abs_path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_vault(&path, false))
        .await
        .map_err(|err| KpError(AppError::IoFailure(format!("后台任务失败：{err}"))))?
        .map_err(KpError)?;
    activate_vault(&state, prepared).map_err(KpError)
}

/// 新建 Vault：路径不存在时创建目录，并在其中初始化 .knowlpad/ 与空索引库。
#[tauri::command]
pub async fn vault_create(
    state: tauri::State<'_, AppState>,
    args: VaultCreateArgs,
) -> Result<VaultInfo, KpError> {
    let path = PathBuf::from(&args.abs_path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_vault(&path, true))
        .await
        .map_err(|err| KpError(AppError::IoFailure(format!("后台任务失败：{err}"))))?
        .map_err(KpError)?;
    let info = activate_vault(&state, prepared).map_err(KpError)?;
    if let (Some(name), Some(id), Some(global)) = (args.name, info.vault_id, state.global_db()) {
        crate::storage::global::rename_vault(&global, id, &name).map_err(KpError)?;
        return Ok(VaultInfo {
            display_name: name,
            ..info
        });
    }
    Ok(info)
}

/// 关闭当前 Vault（先刷盘）。
#[tauri::command]
pub async fn vault_close(state: tauri::State<'_, AppState>) -> Result<(), KpError> {
    close_current(&state);
    Ok(())
}

/// 当前 Vault 信息（未打开时返回 null）。
#[tauri::command]
pub async fn vault_current(
    state: tauri::State<'_, AppState>,
) -> Result<Option<VaultInfo>, KpError> {
    Ok(current_vault(&state))
}

/// 已注册 Vault 列表（来自全局库；置顶优先，其次最近打开）。
#[tauri::command]
pub async fn vault_list(state: tauri::State<'_, AppState>) -> Result<Vec<VaultSummary>, KpError> {
    let Some(global) = state.global_db() else {
        return Ok(Vec::new());
    };
    let rows = crate::storage::global::list_vaults(&global).map_err(KpError)?;
    Ok(rows
        .into_iter()
        .map(|row| VaultSummary {
            id: row.id,
            abs_path: row.abs_path,
            display_name: row.display_name,
            last_opened: row.last_opened,
            pinned: row.pinned,
        })
        .collect())
}

/// 从列表移除：**仅删除注册记录，绝不删除磁盘文件**（AC-VAULT-04）。
#[tauri::command]
pub async fn vault_register_remove(
    state: tauri::State<'_, AppState>,
    args: VaultIdArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::remove_vault(&global, args.vault_id).map_err(KpError)?;
    // 若移除的是当前打开的 Vault，则同时关闭（但不触碰磁盘）
    if state.current_vault_id() == Some(args.vault_id) {
        close_current(&state);
    }
    Ok(())
}

/// 置顶 / 取消置顶（FR-VAULT-07；PRD 勘误 D-10 补齐的命令契约）。
#[tauri::command]
pub async fn vault_pin(
    state: tauri::State<'_, AppState>,
    args: VaultPinArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::set_pinned(&global, args.vault_id, args.pinned).map_err(KpError)?;
    Ok(())
}

/// 修改显示名。
#[tauri::command]
pub async fn vault_rename(
    state: tauri::State<'_, AppState>,
    args: VaultRenameArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::db("访问知识库注册表")));
    };
    crate::storage::global::rename_vault(&global, args.vault_id, &args.display_name)
        .map_err(KpError)?;
    Ok(())
}

/// 路径失效后重新定位（FR-VAULT-08）：新路径必须存在且可访问。
#[tauri::command]
pub async fn vault_relocate(
    state: tauri::State<'_, AppState>,
    args: VaultRelocateArgs,
) -> Result<VaultInfo, KpError> {
    let path = PathBuf::from(&args.new_abs_path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_vault(&path, false))
        .await
        .map_err(|err| KpError(AppError::IoFailure(format!("后台任务失败：{err}"))))?
        .map_err(KpError)?;

    let abs_path = prepared.root.to_string_lossy().to_string();
    if let Some(global) = state.global_db() {
        crate::storage::global::relocate_vault(&global, args.vault_id, &abs_path)
            .map_err(KpError)?;
    }
    let mut info = activate_vault(&state, prepared).map_err(KpError)?;
    info.vault_id = Some(args.vault_id);
    state.set_current_vault_id(Some(args.vault_id));
    Ok(info)
}

/// 索引状态：已打开 Vault 时读取索引库 meta；未打开时返回布局信息与 ready=false。
#[tauri::command]
pub async fn index_status(state: tauri::State<'_, AppState>) -> Result<IndexStatus, KpError> {
    let index_dir = crate::storage::INDEX_DIR_REL.to_string();
    let index_db = crate::storage::INDEX_DB_REL.to_string();

    let Some(pool) = state.index_db() else {
        return Ok(IndexStatus {
            index_dir,
            index_db,
            ready: false,
            schema_version: None,
            signature_prefix: None,
            built_at: None,
        });
    };

    let (schema_version, signature, built_at) = pool
        .with_reader(|conn| {
            Ok((
                crate::storage::index::read_meta(conn, "schema_version")?,
                crate::storage::index::read_meta(conn, "index_signature")?,
                crate::storage::index::read_meta(conn, "built_at")?,
            ))
        })
        .map_err(KpError)?;

    Ok(IndexStatus {
        index_dir,
        index_db,
        ready: true,
        schema_version: schema_version.and_then(|raw| raw.parse().ok()),
        signature_prefix: signature.map(|raw| raw.chars().take(12).collect()),
        built_at: built_at.and_then(|raw| raw.parse().ok()),
    })
}

#[cfg(test)]
#[path = "vault_tests.rs"]
mod tests;
