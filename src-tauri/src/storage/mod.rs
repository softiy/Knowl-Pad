//! 存储层：SQL 的唯一所在地（RS-03）。
//!
//! M1 落地内容（技术方案 §4）：连接管理（§4.1）、PRAGMA 配置（§4.2）、事务模式（§4.3）、
//! 全局库版本化迁移（§4.5）。索引库 schema 与「丢弃重建」在 PR-2。

pub mod global;
pub mod index;
pub mod index_rebuild;
pub mod index_schema;
pub mod migrate;
pub mod pool;
pub mod pragma;

use kp_domain::error::AppError;
use std::path::PathBuf;

/// 索引库文件名（相对 Vault 根）。
pub const INDEX_DB_REL: &str = ".knowlpad/index.db";

/// 索引库所在目录（相对 Vault 根）。
pub const INDEX_DIR_REL: &str = ".knowlpad";

/// 全局库文件名（位于平台配置目录内；路径必须经 Tauri path API 获取，禁止硬编码）。
pub const GLOBAL_DB_FILE: &str = "global.db";

/// 启动诊断信息：确认 PRAGMA 已生效，并验证 bundled SQLite 是否启用 FTS5（TR-03）。
#[derive(Debug, Clone)]
pub struct Diagnostics {
    pub path: PathBuf,
    pub journal_mode: String,
    pub foreign_keys: bool,
    pub fts5: bool,
    pub compile_option_count: usize,
}

/// 采集诊断信息（只读，不影响库状态）。
pub fn diagnostics(pool: &pool::DbPool) -> Result<Diagnostics, AppError> {
    let (journal_mode, foreign_keys, options) = pool.with_reader(|conn| {
        Ok((
            pragma::journal_mode(conn)?,
            pragma::foreign_keys_enabled(conn)?,
            pragma::compile_options(conn)?,
        ))
    })?;
    let fts5 = options.iter().any(|opt| opt.contains("ENABLE_FTS5"));
    Ok(Diagnostics {
        path: pool.path().to_path_buf(),
        journal_mode,
        foreign_keys,
        fts5,
        compile_option_count: options.len(),
    })
}
