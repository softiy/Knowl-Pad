use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::path_guard::PathGuard;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultOpenArgs {
    pub abs_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultCreateArgs {
    pub abs_path: String,
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultIdArgs {
    pub vault_id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRenameArgs {
    pub vault_id: i64,
    pub display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRelocateArgs {
    pub vault_id: i64,
    pub new_abs_path: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub root: String,
    pub display_name: String,
    /// 全局库中的注册 id（全局库不可用时为 None）
    pub vault_id: Option<i64>,
    pub case_insensitive_fs: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened: Option<i64>,
    pub pinned: bool,
}

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

    Ok(VaultInfo {
        root: abs_path,
        display_name: prepared.display_name,
        vault_id,
        case_insensitive_fs: crate::platform::case_insensitive_fs(),
    })
}

/// 关闭当前 Vault：WAL 刷盘 → 释放索引库句柄 → 清空状态（FR-VAULT-05）。
pub(crate) fn close_current(state: &AppState) {
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
        return Err(KpError(AppError::DbError("全局库不可用".into())));
    };
    crate::storage::global::remove_vault(&global, args.vault_id).map_err(KpError)?;
    // 若移除的是当前打开的 Vault，则同时关闭（但不触碰磁盘）
    if state.current_vault_id() == Some(args.vault_id) {
        close_current(&state);
    }
    Ok(())
}

/// 修改显示名。
#[tauri::command]
pub async fn vault_rename(
    state: tauri::State<'_, AppState>,
    args: VaultRenameArgs,
) -> Result<(), KpError> {
    let Some(global) = state.global_db() else {
        return Err(KpError(AppError::DbError("全局库不可用".into())));
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub index_dir: String,
    pub index_db: String,
    pub ready: bool,
    /// 索引库 schema 版本（未就绪时为 None）
    pub schema_version: Option<i64>,
    /// 索引签名摘要前 12 位（用于 UI 诊断；未就绪时为 None）
    pub signature_prefix: Option<String>,
    /// 上次建库时间戳（毫秒）
    pub built_at: Option<i64>,
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
mod tests {
    use super::*;

    /// 建一个带全局库的 AppState（全局库落在独立临时目录）。
    fn state_with_global() -> (tempfile::TempDir, AppState) {
        let cfg = tempfile::tempdir().expect("配置目录应可创建");
        let global = crate::storage::global::open(cfg.path()).expect("全局库应可打开");
        let state = AppState::new();
        state.set_global_db(Arc::new(global));
        (cfg, state)
    }

    #[test]
    fn invalid_path_errors_without_creating_anything() {
        // AC-VAULT-02：路径不存在时必须报 E_VAULT_PATH_INVALID，且**不创建任何目录**
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let missing = base.path().join("not-there");
        let err = prepare_vault(&missing, false)
            .err()
            .expect("路径不存在必须报错");
        assert_eq!(err.code(), "E_VAULT_PATH_INVALID");
        assert!(!missing.exists(), "失败路径上不得创建目录");
    }

    #[test]
    fn create_initializes_knowlpad_and_index_db() {
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let target = base.path().join("new-vault");
        let prepared = prepare_vault(&target, true).expect("新建应成功");
        assert!(target.join(".knowlpad").is_dir(), "应创建 .knowlpad/");
        assert!(
            target.join(".knowlpad").join("index.db").is_file(),
            "应创建索引库"
        );
        assert_eq!(prepared.display_name, "new-vault");
    }

    #[cfg(unix)]
    #[test]
    fn knowlpad_dir_has_owner_only_permissions() {
        // FR-STORAGE-01：.knowlpad/ 权限为仅当前用户可读写（0700）
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let target = base.path().join("perm-vault");
        let _prepared = prepare_vault(&target, true).expect("新建应成功");
        let mode = std::fs::metadata(target.join(".knowlpad"))
            .expect("应可读取目录元数据")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "实际权限为 {mode:o}");
    }

    #[test]
    fn activate_registers_in_global_and_sets_state() {
        let (_cfg, state) = state_with_global();
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let target = base.path().join("vault-a");
        let prepared = prepare_vault(&target, true).expect("新建应成功");
        let info = activate_vault(&state, prepared).expect("激活应成功");

        assert!(info.vault_id.is_some(), "应注册到全局库");
        assert!(state.current_root().is_some());
        assert!(state.index_db().is_some());
        assert_eq!(state.current_vault_id(), info.vault_id);

        let global = state.global_db().expect("全局库应可用");
        let rows = crate::storage::global::list_vaults(&global).expect("列表应成功");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].display_name, "vault-a");
    }

    #[test]
    fn switching_vault_closes_previous_and_keeps_both_registered() {
        // AC-VAULT-03（存储侧）：切换时前一个 Vault 的连接被关闭、状态被替换
        let (_cfg, state) = state_with_global();
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let a = base.path().join("vault-a");
        let b = base.path().join("vault-b");

        let info_a =
            activate_vault(&state, prepare_vault(&a, true).expect("准备 A")).expect("激活 A");
        let info_b =
            activate_vault(&state, prepare_vault(&b, true).expect("准备 B")).expect("激活 B");

        assert_ne!(info_a.root, info_b.root);
        assert_eq!(
            state
                .current_root()
                .map(|p| p.to_string_lossy().to_string()),
            Some(info_b.root.clone()),
            "状态应指向 B"
        );
        let global = state.global_db().expect("全局库应可用");
        assert_eq!(
            crate::storage::global::list_vaults(&global)
                .expect("列表应成功")
                .len(),
            2,
            "两个 Vault 都应留在注册表中"
        );
    }

    #[test]
    fn current_vault_reflects_open_and_closed() {
        let (_cfg, state) = state_with_global();
        assert!(current_vault(&state).is_none(), "未打开时应为 None");
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let target = base.path().join("vault-c");
        let prepared = prepare_vault(&target, true).expect("准备应成功");
        activate_vault(&state, prepared).expect("激活应成功");
        let info = current_vault(&state).expect("应返回当前 Vault");
        assert!(info.root.ends_with("vault-c"));
        close_current(&state);
        assert!(current_vault(&state).is_none(), "关闭后应为 None");
        assert!(state.index_db().is_none(), "关闭后索引库句柄应释放");
    }

    #[test]
    fn remove_from_list_keeps_files_on_disk() {
        // AC-VAULT-04（命令侧）：移除注册记录后磁盘文件必须原样保留
        let (_cfg, state) = state_with_global();
        let base = tempfile::tempdir().expect("临时目录应可创建");
        let target = base.path().join("vault-d");
        std::fs::create_dir_all(&target).expect("应可创建目录");
        let note = target.join("note.md");
        std::fs::write(&note, "# 保留我").expect("应可写笔记");

        let info = activate_vault(&state, prepare_vault(&target, false).expect("准备"))
            .expect("激活应成功");
        let id = info.vault_id.expect("应有注册 id");
        let global = state.global_db().expect("全局库应可用");
        assert!(crate::storage::global::remove_vault(&global, id).expect("移除应成功"));
        assert!(note.exists(), "磁盘文件必须保留");
    }
}
