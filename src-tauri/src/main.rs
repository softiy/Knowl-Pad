#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dependency_on_unit_never_type_fallback)]
mod commands;
mod error_wrapper;
mod index_engine;
#[cfg(test)]
mod index_engine_tests;
#[cfg(test)]
mod index_reliability_tests;
mod index_resolve;
mod index_watch;
mod index_watch_events;
mod index_watch_paths;
#[cfg(test)]
mod index_watch_tests;
mod logging;
mod platform;
mod state;
mod storage;
use commands::file::{file_list_dir, file_stat, file_tree, file_validate_name};
use commands::file_write::{
    file_delete, file_move, file_rename, file_reveal, folder_create, note_create,
};
use commands::index::{index_cancel, index_rebuild, index_signature_get, index_stats};
use commands::link::{
    link_ambiguous_list, link_backlinks, link_dangling_list, link_headings, link_orphan_list,
    link_outgoing,
};
use commands::link_resolve::link_resolve_ambiguous;
use commands::link_rewrite::{link_rewrite_apply, link_rewrite_preview};
use commands::link_rewrite_store::link_rewrite_rollback;
use commands::note::{note_read, note_write};
use commands::settings::{preference_get, preference_set, vault_state_get, vault_state_set};
use commands::system::{ping, system_info};
use commands::vault::{
    index_status, vault_close, vault_create, vault_current, vault_list, vault_open, vault_pin,
    vault_register_remove, vault_relocate, vault_rename,
};
use state::AppState;
fn main() {
    let smoke = std::env::var("KP_SMOKE").is_ok();
    tauri::Builder::default()
        .setup(move |app| {
            // 全局库（global.db）：配置目录必须经 Tauri path API 取得（PRD §2.4 实现约束）
            {
                use tauri::Manager;
                match app.path().app_config_dir() {
                    Ok(dir) => {
                        // 但**日志失败不得阻断存储层**——两者是独立的降级单元。
                        if let Err(err) = logging::init(&dir.join("logs")) {
                            eprintln!("日志初始化失败（继续运行）：{err}");
                        } else if logging::is_initialized() {
                            tracing::info!(
                                version = env!("CARGO_PKG_VERSION"),
                                platform = std::env::consts::OS,
                                "Knowl Pad 启动"
                            );
                        }
                        match storage::global::open(&dir) {
                            Ok(pool) => {
                                match storage::diagnostics(&pool) {
                                    Ok(info) => tracing::info!(
                                        path = %info.path.display(),
                                        journal_mode = %info.journal_mode,
                                        foreign_keys = info.foreign_keys,
                                        fts5 = info.fts5,
                                        compile_options = info.compile_option_count,
                                        "全局库已就绪"
                                    ),
                                    Err(err) => tracing::warn!(error = %err, "全局库诊断失败"),
                                }
                                if let Err(err) = storage::global::record_startup(&pool) {
                                    tracing::warn!(error = %err, "启动记录写入失败（不影响使用）");
                                }
                                app.state::<AppState>()
                                    .set_global_db(std::sync::Arc::new(pool));
                                // FR-VAULT-06：恢复上次打开的 Vault（失败或路径失效都不阻断启动，
                                // 且绝不创建目录——AC-VAULT-02）
                                if commands::vault_lifecycle::restore_last_vault(
                                    &app.state::<AppState>(),
                                )
                                .is_some()
                                {
                                    commands::vault::reconcile_after_open(
                                        &app.state::<AppState>(),
                                        app.handle(),
                                    );
                                }
                                tracing::debug!(
                                    ready = app.state::<AppState>().global_db().is_some(),
                                    "全局库句柄已登记"
                                );
                            }
                            // FR-GLOBAL-01：全局库损坏或不可用时，以默认配置继续启动，不崩溃、不丢笔记
                            Err(err) => {
                                tracing::warn!(error = %err, "全局库不可用，以默认配置继续")
                            }
                        }
                    }
                    Err(err) => tracing::warn!(error = %err, "无法取得配置目录，跳过全局库"),
                }
            }
            if smoke {
                use tauri::Manager;
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                    loop {
                        if let Some(win) = handle.get_webview_window("main") {
                            let visible = win.is_visible().unwrap_or(false);
                            println!("KP_SMOKE_OK window=main visible={visible}");
                            handle.exit(0);
                            return;
                        }
                        if std::time::Instant::now() > deadline {
                            eprintln!("KP_SMOKE_FAIL: main window not created within 20s");
                            handle.exit(1);
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                });
            }
            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            preference_get,
            preference_set,
            vault_state_get,
            vault_state_set,
            ping,
            system_info,
            note_create,
            folder_create,
            file_rename,
            file_move,
            file_delete,
            file_reveal,
            file_tree,
            file_list_dir,
            file_stat,
            file_validate_name,
            note_read,
            note_write,
            vault_open,
            vault_create,
            vault_current,
            vault_list,
            vault_register_remove,
            vault_pin,
            vault_rename,
            vault_relocate,
            index_status,
            link_backlinks,
            link_outgoing,
            link_dangling_list,
            link_ambiguous_list,
            link_orphan_list,
            link_headings,
            link_rewrite_preview,
            link_rewrite_apply,
            link_rewrite_rollback,
            link_resolve_ambiguous,
            index_signature_get,
            index_stats,
            index_rebuild,
            index_cancel,
            vault_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running Knowl Pad");
}
#[cfg(test)]
mod appendix_b_tests;
#[cfg(test)]
mod m0_stub_tests {
    use super::*;
    #[test]
    fn platform_flag_is_defined() {
        let _ = platform::case_insensitive_fs();
    }
    #[test]
    fn storage_constants_are_stable() {
        assert_eq!(storage::INDEX_DIR_REL, ".knowlpad");
        assert_eq!(storage::INDEX_DB_REL, ".knowlpad/index.db");
    }
    #[test]
    fn vault_root_lifecycle() {
        let state = AppState::new();
        assert!(state.current_root().is_none(), "未打开 Vault 时应为空");
        let dir = tempfile::tempdir().unwrap();
        state.set_root(Some(dir.path().to_path_buf()));
        assert!(state.current_root().is_some(), "打开 Vault 后应有值");
        state.set_root(None);
        assert!(state.current_root().is_none(), "关闭后应再次为空");
    }
}
