//! 存储层占位：SQL 的唯一所在地（RS-03）。
//! M0 仅保留结构；索引库 schema、连接池与迁移在 M1 落地（PRD §3.2、技术方案 §4）。

/// 索引库文件名（相对 Vault 根）。
pub const INDEX_DB_REL: &str = ".knowlpad/index.db";

/// 索引库所在目录（相对 Vault 根）。
pub const INDEX_DIR_REL: &str = ".knowlpad";
