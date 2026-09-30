//! Tauri 侧 crate：领域逻辑在 `kp-domain`（无 Tauri 依赖，可独立单测与度量覆盖率）。
//! 需要桌面框架的装配与 IPC 薄壳放在 bin（main.rs + commands/state/error_wrapper）。

pub use kp_domain as domain;
pub use kp_domain::error;
