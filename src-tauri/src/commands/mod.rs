pub mod file;
pub mod file_index_view;
pub mod file_write;
pub mod index;
#[cfg(test)]
mod index_tests;
pub mod link;
pub mod link_list;
pub mod link_resolve;
pub mod link_rewrite;
pub mod link_rewrite_store;
#[cfg(test)]
mod link_rewrite_tests;
#[cfg(test)]
mod link_tests;
mod link_text;
mod link_view;
pub mod note;
pub mod paths;
pub mod search;
mod search_file;
// search_index / search_page 需要 `pub(crate)`：M5 批次 5 的 D25 度量用例在 crate 根的
// perf_d25_tests 里，要按命令入口（search_fulltext_impl）与组装路径（page_with_line_filter）真测；
// 它们仍是 crate 内部模块，不对外暴露。
pub(crate) mod search_index;
pub(crate) mod search_page;
#[cfg(test)]
mod search_tests;
mod search_view;
pub mod settings;
pub mod system;
pub mod tag;
#[cfg(test)]
mod tag_tests;
mod tag_view;
pub mod vault;
pub mod vault_lifecycle;
pub mod vault_types;
