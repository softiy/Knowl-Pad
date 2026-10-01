//! Vault 命令与生命周期的单元测试
//! （CODE-11：测试位于独立文件，不占用实现文件行数上限）

use super::*;
use crate::commands::vault_lifecycle::*;
use std::sync::Arc;

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

    let info_a = activate_vault(&state, prepare_vault(&a, true).expect("准备 A")).expect("激活 A");
    let info_b = activate_vault(&state, prepare_vault(&b, true).expect("准备 B")).expect("激活 B");

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

    let info =
        activate_vault(&state, prepare_vault(&target, false).expect("准备")).expect("激活应成功");
    let id = info.vault_id.expect("应有注册 id");
    let global = state.global_db().expect("全局库应可用");
    assert!(crate::storage::global::remove_vault(&global, id).expect("移除应成功"));
    assert!(note.exists(), "磁盘文件必须保留");
}
/// 模拟「进程重启」：仅清空内存态，**不**走 close_current（那是用户显式关闭语义）。
fn simulate_restart(state: &AppState) {
    state.set_index_db(None);
    state.set_root(None);
    state.set_current_vault_id(None);
}

#[test]
fn restore_reopens_last_vault() {
    let (_cfg, state) = state_with_global();
    let base = tempfile::tempdir().expect("临时目录应可创建");
    let target = base.path().join("vault-restore");
    let info = activate_vault(&state, prepare_vault(&target, true).expect("准备应成功"))
        .expect("激活应成功");
    let id = info.vault_id.expect("应有注册 id");

    simulate_restart(&state);
    assert!(state.current_root().is_none(), "重启后内存态应为空");
    let restored = restore_last_vault(&state).expect("应恢复上次 Vault");
    assert_eq!(restored.vault_id, Some(id), "恢复的应是同一个注册项");
    assert!(state.current_root().is_some(), "恢复后应处于打开状态");
}

#[test]
fn restore_skips_after_user_closed_vault() {
    let (_cfg, state) = state_with_global();
    let base = tempfile::tempdir().expect("临时目录应可创建");
    let target = base.path().join("vault-closed");
    activate_vault(&state, prepare_vault(&target, true).expect("准备应成功")).expect("激活应成功");
    close_current(&state); // 用户显式关闭 → 不应再自动恢复
    simulate_restart(&state);
    assert!(
        restore_last_vault(&state).is_none(),
        "用户已关闭的 Vault 不应被恢复"
    );
}

#[test]
fn restore_skips_missing_path_without_creating_dirs() {
    // AC-VAULT-02：路径失效时不得创建空目录，也不得崩溃。
    // 说明：此处直接构造「注册表指向不存在路径」的记录，而不是删除一个已打开的 Vault
    // （后者在 Windows 上会因索引库句柄未释放而删不掉——见审查发现 #8，另案修复）。
    let (_cfg, state) = state_with_global();
    let base = tempfile::tempdir().expect("临时目录应可创建");
    let missing = base.path().join("vault-gone");
    let global = state.global_db().expect("全局库应可用");
    let id =
        crate::storage::global::upsert_vault(&global, &missing.to_string_lossy(), "已失效的库")
            .expect("注册应成功");
    crate::storage::global::set_last_vault(&global, id).expect("记录应成功");
    simulate_restart(&state);

    assert!(restore_last_vault(&state).is_none(), "路径失效应放弃恢复");
    assert!(!missing.exists(), "失效路径上不得创建任何目录");
    assert!(state.current_root().is_none(), "不得进入打开状态");
}

#[test]
fn restore_respects_disabled_setting() {
    let (_cfg, state) = state_with_global();
    let base = tempfile::tempdir().expect("临时目录应可创建");
    let target = base.path().join("vault-nosrestore");
    activate_vault(&state, prepare_vault(&target, true).expect("准备应成功")).expect("激活应成功");
    let global = state.global_db().expect("全局库应可用");
    crate::storage::global::set_preference(
        &global,
        crate::storage::global::PREF_RESTORE_LAST,
        "false",
    )
    .expect("关闭开关应成功");

    simulate_restart(&state);
    assert!(restore_last_vault(&state).is_none(), "关闭后不应恢复");
}
