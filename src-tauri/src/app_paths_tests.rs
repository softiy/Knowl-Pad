//! `KP_CONFIG_DIR` 的语义用例（D-25 ④⑤：debug-only 的测试隔离开关）。

use std::path::PathBuf;
use std::sync::Mutex;

use super::{resolve_config_dir, CONFIG_DIR_ENV};

/// 环境变量是进程级的：两条用例必须串行，否则互相踩。
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn unset_uses_default_config_dir() {
    let _guard = ENV_LOCK.lock().expect("环境锁");
    std::env::remove_var(CONFIG_DIR_ENV);
    let default = PathBuf::from("C:/default-config");
    assert_eq!(resolve_config_dir(default.clone()), default);
}

#[test]
fn debug_build_honors_kp_config_dir() {
    let _guard = ENV_LOCK.lock().expect("环境锁");
    std::env::set_var(CONFIG_DIR_ENV, "C:/temp/isolated-config");
    let got = resolve_config_dir(PathBuf::from("C:/default-config"));
    std::env::remove_var(CONFIG_DIR_ENV);
    // 用例只在 debug 构建里跑（cargo test 默认 debug）⇒ 期望被覆盖
    assert_eq!(got, PathBuf::from("C:/temp/isolated-config"));
}

#[test]
fn empty_value_is_ignored() {
    let _guard = ENV_LOCK.lock().expect("环境锁");
    std::env::set_var(CONFIG_DIR_ENV, "");
    let default = PathBuf::from("C:/default-config");
    let got = resolve_config_dir(default.clone());
    std::env::remove_var(CONFIG_DIR_ENV);
    assert_eq!(got, default, "空值不得把配置目录指到空路径");
}
