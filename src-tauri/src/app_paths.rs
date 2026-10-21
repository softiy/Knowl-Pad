//! 应用路径的解析（**测试隔离**用）。
//!
//! 为什么需要它：启动路径会打开系统配置目录的 `global.db`（注册表/`last_vault`）并 `restore_last_vault`，
//! 在开发者机器上那可能是真实用户的 Vault（甚至是冒烟 Vault）。要给 NFR-PERF-13/14 做进程级采样，必须有办法把
//! 配置目录指到 `%TEMP%` 下的临时目录 —— 否则"采样"就会碰真实数据。
//!
//! 安全约束（D-25 裁定的要求）：**仅调试构建生效**。release 构建里 `cfg!(debug_assertions)` 为 false，
//! 分支被编译期折叠掉，环境变量即使设置也**不会**被读取。

use std::path::PathBuf;

/// 环境变量名（调试构建下把配置目录指到别处；用于测试隔离）。
pub const CONFIG_DIR_ENV: &str = "KP_CONFIG_DIR";

/// 解析配置目录：调试构建认 `KP_CONFIG_DIR`（非空），其余一律用系统默认目录。
pub fn resolve_config_dir(default_dir: PathBuf) -> PathBuf {
    if cfg!(debug_assertions) {
        if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
            let path = PathBuf::from(dir);
            if !path.as_os_str().is_empty() {
                return path;
            }
        }
    }
    default_dir
}

#[cfg(test)]
#[path = "app_paths_tests.rs"]
mod tests;
