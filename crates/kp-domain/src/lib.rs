//! Knowl Pad 领域层（纯逻辑，无 Tauri 依赖）。
//! 独立成 crate 有两个目的：① 满足 RS-02（可脱离桌面环境单测）；
//! ② 依赖树中**不含任何带构建脚本的 crate**，使 clippy/test/coverage 可在受限环境中本地执行。

pub mod error;
pub mod fs_atomic;
pub mod note_io;
pub mod path_guard;
pub mod vault_paths;
