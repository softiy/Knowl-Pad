//! Knowl Pad 领域层（纯逻辑，无 Tauri 依赖）。
//! 独立成 crate 有两个目的：① 满足 RS-02（可脱离桌面环境单测）；
//! ② 依赖树中**不含任何带构建脚本的 crate**，使 clippy/test/coverage 可在受限环境中本地执行。

pub mod error;
pub mod file_ops;
pub mod file_trash;
pub mod file_tree;
pub mod find_in_file;
pub mod fs_atomic;
pub mod link_rewrite;
pub mod link_rewrite_apply;
pub mod link_rewrite_diff;
pub mod link_rewrite_protect;
pub mod link_rewrite_single;
pub mod md_parse;
pub mod note_io;
pub mod path_guard;
pub mod quick_open;
pub mod search;
pub mod search_history;
pub mod search_hits;
pub mod search_syntax;
pub mod snippet;
pub mod tag_rewrite;
pub mod tag_tree;
pub mod tokenize;
pub mod tokenizer_meta;
pub mod vault_paths;
