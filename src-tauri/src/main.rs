// 隐藏 Windows 下的控制台窗口（release）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Tauri 的 #[tauri::command] 宏展开依赖 never type fallback（Rust 2024 兼容性 lint，
// 现为 deny-by-default）。此处显式允许；待 Tauri 宏修复后移除。
#![allow(dependency_on_unit_never_type_fallback)]

mod commands;
mod error_wrapper;
mod platform;
mod state;
mod storage;

use commands::note::{note_read, note_write};
use commands::system::{ping, system_info};
use commands::vault::{
    index_status, vault_close, vault_create, vault_current, vault_list, vault_open,
    vault_register_remove, vault_relocate, vault_rename,
};
use state::AppState;

fn main() {
    // 平台冒烟模式：KP_SMOKE=1 时，窗口创建后自行断言并在超时内退出。
    // 供 CI（Linux/xvfb、macOS、Windows runner）验证「应用能启动且主窗口存在」。
    let smoke = std::env::var("KP_SMOKE").is_ok();

    tauri::Builder::default()
        .setup(move |app| {
            // 全局库（global.db）：配置目录必须经 Tauri path API 取得（PRD §2.4 实现约束）
            {
                use tauri::Manager;
                match app.path().app_config_dir() {
                    Ok(dir) => match storage::global::open(&dir) {
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
                            tracing::debug!(
                                ready = app.state::<AppState>().global_db().is_some(),
                                "全局库句柄已登记"
                            );
                        }
                        // FR-GLOBAL-01：全局库损坏或不可用时，以默认配置继续启动，不崩溃、不丢笔记
                        Err(err) => tracing::warn!(error = %err, "全局库不可用，以默认配置继续"),
                    },
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
            ping,
            system_info,
            note_read,
            note_write,
            vault_open,
            vault_create,
            vault_current,
            vault_list,
            vault_register_remove,
            vault_rename,
            vault_relocate,
            index_status,
            vault_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running Knowl Pad");
}

#[cfg(test)]
mod m0_stub_tests {
    use super::*;

    #[test]
    fn platform_flag_is_defined() {
        // 至少调用一次，保证平台适配入口可用且不被 dead_code 判定
        let _ = platform::case_insensitive_fs();
    }

    #[test]
    fn storage_constants_are_stable() {
        assert_eq!(storage::INDEX_DIR_REL, ".knowlpad");
        assert_eq!(storage::INDEX_DB_REL, ".knowlpad/index.db");
    }

    #[test]
    fn vault_root_lifecycle() {
        // 路径校验职责已下沉到 storage::index::open / PathGuard（PR-2），
        // 此处只断言 AppState 对根路径的持有语义。
        let state = AppState::new();
        assert!(state.current_root().is_none(), "未打开 Vault 时应为空");

        let dir = tempfile::tempdir().unwrap();
        state.set_root(Some(dir.path().to_path_buf()));
        assert!(state.current_root().is_some(), "打开 Vault 后应有值");

        state.set_root(None);
        assert!(state.current_root().is_none(), "关闭后应再次为空");
    }
}
