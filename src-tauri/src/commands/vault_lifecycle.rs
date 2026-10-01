//! Vault 生命周期核心逻辑（与 Tauri 解耦，便于单测；命令层只做参数搬运）。

use super::vault_types::VaultInfo;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::path_guard::PathGuard;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── 核心逻辑（与 Tauri 解耦，便于单测；命令层只做参数搬运）──────────────

/// 已准备就绪的 Vault（目录、.knowlpad/ 、索引库均已就位），等待激活。
pub(crate) struct PreparedVault {
    pub root: PathBuf,
    pub display_name: String,
    pub pool: crate::storage::pool::DbPool,
}

/// 准备 Vault：校验路径 → （可选）建目录 → 确保 .knowlpad/ → 打开索引库。
///
/// **阻塞 IO**，调用方应放入线程池（R-08）。
/// 路径无效时**绝不创建任何目录**（FR-VAULT-08 / AC-VAULT-02）。
pub(crate) fn prepare_vault(
    path: &Path,
    create_if_missing: bool,
) -> Result<PreparedVault, AppError> {
    if !path.exists() {
        if !create_if_missing {
            return Err(AppError::VaultPathInvalid(
                path.to_string_lossy().to_string(),
            ));
        }
        std::fs::create_dir_all(path)?;
    }
    if !path.is_dir() {
        return Err(AppError::VaultPathInvalid(format!(
            "{} 不是目录",
            path.to_string_lossy()
        )));
    }
    // 规范化（符号链接、大小写、Windows 前缀等；SEC-02 前置步骤）
    let guard = PathGuard::new(path)?;
    let root = guard.canonical_root().to_path_buf();
    ensure_knowlpad_dir(&root)?;

    // §6.3 启动清理：删除上次崩溃/强杀遗留的 .kp-tmp-*（失败不影响打开）
    match kp_domain::note_io::cleanup_temp_files_recursive(&root) {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, "已清理临时文件残留"),
        Err(err) => tracing::warn!(error = %err, "清理临时文件残留失败"),
    }

    let display_name = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "Vault".to_string());

    let pool = crate::storage::index::open(&root)?;
    Ok(PreparedVault {
        root,
        display_name,
        pool,
    })
}

/// 确保 .knowlpad/ 存在且权限收敛（FR-STORAGE-01：仅当前用户可读写）。
fn ensure_knowlpad_dir(root: &Path) -> Result<(), AppError> {
    let dir = root.join(crate::storage::INDEX_DIR_REL);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    // Windows 无 POSIX 权限位；目录位于用户 Vault 内，继承父目录 ACL。
    Ok(())
}

/// 激活 Vault：先关闭当前 Vault（刷盘 + 释放连接）→ 注册到全局库 → 写入 AppState。
pub(crate) fn activate_vault(
    state: &AppState,
    prepared: PreparedVault,
) -> Result<VaultInfo, AppError> {
    close_current(state);

    // §9.5 日志内容红线：Vault 绝对路径**仅在此处记录一次**（用于诊断），
    // 其余日志只允许出现相对路径
    tracing::info!(root = %prepared.root.display(), "打开 Vault");

    let abs_path = prepared.root.to_string_lossy().to_string();
    let vault_id = match state.global_db() {
        Some(global) => Some(crate::storage::global::upsert_vault(
            &global,
            &abs_path,
            &prepared.display_name,
        )?),
        None => None,
    };

    state.set_index_db(Some(Arc::new(prepared.pool)));
    state.set_root(Some(prepared.root.clone()));
    state.set_current_vault_id(vault_id);

    // FR-VAULT-06：记住本次打开的 Vault，供下次启动恢复
    if let (Some(id), Some(global)) = (vault_id, state.global_db()) {
        if let Err(err) = crate::storage::global::set_last_vault(&global, id) {
            tracing::warn!(error = %err, "记录上次打开的 Vault 失败（不影响使用）");
        }
    }

    Ok(VaultInfo {
        root: abs_path,
        display_name: prepared.display_name,
        vault_id,
        case_insensitive_fs: crate::platform::case_insensitive_fs(),
    })
}

/// 关闭当前 Vault：WAL 刷盘 → 释放索引库句柄 → 清空状态（FR-VAULT-05）。
pub(crate) fn close_current(state: &AppState) {
    // FR-VAULT-06：用户显式关闭（或切走/移除）后不应再自动恢复
    if let Some(global) = state.global_db() {
        if let Err(err) = crate::storage::global::clear_last_vault(&global) {
            tracing::warn!(error = %err, "清除上次 Vault 记录失败");
        }
    }
    if let Some(pool) = state.index_db() {
        if let Err(err) = pool.checkpoint() {
            // 刷盘失败不阻断关闭：数据仍在 WAL 中，下次打开可恢复
            tracing::warn!(error = %err, "关闭 Vault 前 WAL 刷盘失败");
        }
    }
    state.set_index_db(None);
    state.set_root(None);
    state.set_current_vault_id(None);
}

/// 当前 Vault 信息（未打开时为 None）。
pub(crate) fn current_vault(state: &AppState) -> Option<VaultInfo> {
    let root = state.current_root()?;
    Some(VaultInfo {
        root: root.to_string_lossy().to_string(),
        display_name: root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Vault".to_string()),
        vault_id: state.current_vault_id(),
        case_insensitive_fs: crate::platform::case_insensitive_fs(),
    })
}

/// 启动恢复（FR-VAULT-06）：尝试重新打开上次的 Vault。
///
/// 语义（AC-VAULT-02 前提）：
/// - 用户已在设置中关闭恢复，或没有记录 → 返回 None，不做任何事；
/// - 路径已失效（移动硬盘拔出等）→ **不创建任何目录**，仅记录告警并返回 None，
///   由 UI 引导「重新定位 / 从列表移除」；
/// - 失败绝不阻断启动。
pub(crate) fn restore_last_vault(state: &AppState) -> Option<VaultInfo> {
    let global = state.global_db()?;
    match crate::storage::global::restore_last_enabled(&global) {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!("已关闭「启动恢复上次 Vault」，跳过");
            return None;
        }
        Err(err) => {
            tracing::warn!(error = %err, "读取恢复开关失败，按默认（恢复）处理");
        }
    }
    let row = match crate::storage::global::last_vault(&global) {
        Ok(Some(row)) => row,
        Ok(None) => return None,
        Err(err) => {
            tracing::warn!(error = %err, "读取上次 Vault 失败");
            return None;
        }
    };

    let path = PathBuf::from(&row.abs_path);
    if !path.is_dir() {
        // 不创建目录、不修改注册表——是否「重新定位/移除」由用户决定
        tracing::warn!(vault_id = row.id, "上次 Vault 路径已失效，跳过自动恢复");
        return None;
    }
    match prepare_vault(&path, false).and_then(|prepared| activate_vault(state, prepared)) {
        Ok(info) => {
            tracing::info!(vault_id = row.id, "已恢复上次打开的 Vault");
            Some(info)
        }
        Err(err) => {
            tracing::warn!(error = %err, vault_id = row.id, "恢复上次 Vault 失败");
            None
        }
    }
}
