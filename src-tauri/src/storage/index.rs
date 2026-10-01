//! 索引库 index.db：DDL、签名比对与「丢弃重建」（PRD §3.2 / §3.6 MIG-01；技术方案 §4.4 / §4.6）。
//!
//! 索引库是**纯派生数据**：schema 变更不做在线迁移，直接丢弃重建（MIG-01）。
//! 触发重建的条件：① 库文件不存在（新库） ② schema_version 不一致 ③ 索引签名不一致
//! ④ 存在未完成的 重建标记（FR-SIG-03，中断后可重新开始，绝不把半索引当有效索引）。

use super::index_schema::DDL_V1;
use super::pool::DbPool;
use super::INDEX_DB_REL;
use kp_domain::error::AppError;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::Path;

/// 索引库 schema 版本：**任何 DDL 变更都必须递增**（技术方案 §4.6 版本维护纪律）。
pub const SCHEMA_VERSION: u32 = 1;

/// 解析器版本：M3 落地解析器后开始递增（当前为占位）。
pub const PARSER_VERSION: u32 = 0;

/// 分词器版本：M3 引入 jieba-rs 后填写其 crate 版本。
pub const TOKENIZER_VERSION: &str = "pending-m3";

/// 词典指纹：M3 由 build.rs 对内嵌词典计算 SHA-256 前 16 位（技术方案 §4.6）。
pub const TOKENIZER_DICT_HASH: &str = "pending-m3";

/// meta 中的重建标记键：重建开始时置位，完成后清除（FR-SIG-03 / NFR-REL-07）。
pub const META_REBUILD_IN_PROGRESS: &str = "rebuild_in_progress";

/// 索引签名（技术方案 §4.6）。四项版本任一变化都应使既有索引失效。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSignature {
    pub schema_version: u32,
    pub parser_version: u32,
    pub tokenizer_version: String,
    pub tokenizer_dict_hash: String,
    pub vault_root: String,
    /// 上述字段规范化拼接后的 SHA-256（十六进制）
    pub digest: String,
}

impl IndexSignature {
    /// 按当前版本常量与 Vault 根构造签名。
    pub fn current(vault_root: &Path) -> Self {
        Self::from_parts(
            SCHEMA_VERSION,
            PARSER_VERSION,
            TOKENIZER_VERSION,
            TOKENIZER_DICT_HASH,
            &vault_root.to_string_lossy(),
        )
    }

    /// 由各字段构造并计算 digest（字段以 \n 拼接，值本身不含换行）。
    pub fn from_parts(
        schema_version: u32,
        parser_version: u32,
        tokenizer_version: &str,
        tokenizer_dict_hash: &str,
        vault_root: &str,
    ) -> Self {
        let joined = format!(
            "{schema_version}\n{parser_version}\n{tokenizer_version}\n{tokenizer_dict_hash}\n{vault_root}"
        );
        Self {
            schema_version,
            parser_version,
            tokenizer_version: tokenizer_version.to_string(),
            tokenizer_dict_hash: tokenizer_dict_hash.to_string(),
            vault_root: vault_root.to_string(),
            digest: sha256_hex(&joined),
        }
    }

    /// 与另一签名比较，给出人类可读的差异原因（用于 kp://index/rebuild-required 事件）。
    pub fn diff_reason(&self, other: &Self) -> String {
        let mut diffs = Vec::new();
        if self.schema_version != other.schema_version {
            diffs.push(format!(
                "schema_version {} → {}",
                self.schema_version, other.schema_version
            ));
        }
        if self.parser_version != other.parser_version {
            diffs.push(format!(
                "parser_version {} → {}",
                self.parser_version, other.parser_version
            ));
        }
        if self.tokenizer_version != other.tokenizer_version {
            diffs.push("tokenizer_version 变化".to_string());
        }
        if self.tokenizer_dict_hash != other.tokenizer_dict_hash {
            diffs.push("词典指纹变化".to_string());
        }
        if self.vault_root != other.vault_root {
            diffs.push("vault_root 变化".to_string());
        }
        if diffs.is_empty() {
            "无差异".to_string()
        } else {
            diffs.join("；")
        }
    }
}

/// SHA-256 十六进制（sha2 0.11 的 finalize() 不再实现 LowerHex，需手动编码）。
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 打开索引库：签名一致则直接使用；否则**丢弃重建**（MIG-01）。
pub fn open(vault_root: &Path) -> Result<DbPool, AppError> {
    let db_path = vault_root.join(INDEX_DB_REL);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let expected = IndexSignature::current(vault_root);
    DbPool::open_with(&db_path, move |conn| bootstrap(conn, &expected))
}

/// schema 引导：判定是否需要重建 → 建表（幂等）→ 写入 meta。
///
/// 全过程在**单一事务**内：中断即整体回滚，不会留下半建成的 schema。
/// 另按 FR-SIG-03 在库内维护 `rebuild_in_progress` 标记——索引引擎（M3）在重建内容时复用同一语义。
fn bootstrap(conn: &mut Connection, expected: &IndexSignature) -> Result<(), AppError> {
    let meta_exists = table_exists(conn, "meta")?;
    let stored = stored_signature(conn)?;
    let in_progress = read_meta(conn, META_REBUILD_IN_PROGRESS)?.is_some();

    let needs_rebuild = match &stored {
        // 真新库（连 meta 表都不存在）：直接建 schema
        None if !meta_exists => false,
        // meta 存在但缺 index_signature：**不可信**，必须重建。
        // FR-SIG-01 的语义是「与 meta.index_signature 比对」——缺失显然不等于一致；
        // 若不重建，会把当前签名盖到可能已过期的表结构上（静默错误）。
        None => {
            tracing::warn!("索引库缺少签名记录，按不可信处理并重建");
            true
        }
        // 上次重建被中断：重新开始（FR-SIG-03）
        Some(_) if in_progress => {
            tracing::warn!("检测到未完成的索引重建标记，重新开始");
            true
        }
        // 签名不一致：丢弃重建（FR-SIG-01）
        Some(old) if old.digest != expected.digest => {
            tracing::info!(
                reason = %expected.diff_reason(old),
                "索引签名不一致，丢弃重建"
            );
            true
        }
        Some(_) => false,
    };

    // 阶段 1：确保 meta 存在，使「重建进行中」标记能**独立于重建事务**落盘。
    // FR-SIG-03 要求崩溃后下次打开能看见「上次重建未完成」；若标记与重建同事务，
    // 崩溃回滚会一并抹掉标记，该语义即形同虚设（本 MR 前的实现正是如此）。
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(map_err)?;
    set_meta(conn, META_REBUILD_IN_PROGRESS, "1")?;

    // 阶段 2：丢弃重建 + 建表 + 写 meta，单一事务（中断即整体回滚，不留半建成 schema）
    let tx = conn.transaction().map_err(map_err)?;
    if needs_rebuild {
        drop_all(&tx)?;
    }
    create_schema(&tx)?;
    write_meta(&tx, expected)?;
    tx.commit().map_err(map_err)?;

    // 阶段 3：完成后清除标记
    clear_meta(conn, META_REBUILD_IN_PROGRESS)?;
    Ok(())
}

/// 从 meta 还原上次写入的签名（用于精确的差异原因）。
fn stored_signature(conn: &Connection) -> Result<Option<IndexSignature>, AppError> {
    let Some(digest) = read_meta(conn, "index_signature")? else {
        return Ok(None);
    };
    let parse = |value: Option<String>| value.and_then(|raw| raw.parse::<u32>().ok()).unwrap_or(0);
    Ok(Some(IndexSignature {
        schema_version: parse(read_meta(conn, "schema_version")?),
        parser_version: parse(read_meta(conn, "parser_version")?),
        tokenizer_version: read_meta(conn, "tokenizer_version")?.unwrap_or_default(),
        tokenizer_dict_hash: read_meta(conn, "tokenizer_dict_hash")?.unwrap_or_default(),
        vault_root: read_meta(conn, "vault_root")?.unwrap_or_default(),
        digest,
    }))
}

/// 建表（全部 IF NOT EXISTS，可重复执行）。
fn create_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(DDL_V1).map_err(map_err)
}

/// 丢弃全部表。顺序遵循外键依赖：先子表后父表；FTS5 影子表随虚拟表一并删除。
fn drop_all(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS file_tag;
         DROP TABLE IF EXISTS file_alias;
         DROP TABLE IF EXISTS link;
         DROP TABLE IF EXISTS block_id;
         DROP TABLE IF EXISTS heading;
         DROP TABLE IF EXISTS tag;
         DROP TABLE IF EXISTS note_fts;
         DROP TABLE IF EXISTS file;
         DROP TABLE IF EXISTS meta;",
    )
    .map_err(map_err)
}

/// 写入签名与元信息（schema_version / index_signature / vault_root / parser_version / built_at）。
fn write_meta(conn: &Connection, signature: &IndexSignature) -> Result<(), AppError> {
    let built_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let entries: [(&str, String); 5] = [
        ("schema_version", signature.schema_version.to_string()),
        ("index_signature", signature.digest.clone()),
        ("vault_root", signature.vault_root.clone()),
        ("parser_version", signature.parser_version.to_string()),
        ("built_at", built_at.to_string()),
    ];
    for (key, value) in entries {
        conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES(?1, ?2)",
            [key, value.as_str()],
        )
        .map_err(map_err)?;
    }
    Ok(())
}

/// 读取 meta 值；表不存在或无该键时返回 None。
pub fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    if !table_exists(conn, "meta")? {
        return Ok(None);
    }
    match conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
        row.get::<_, String>(0)
    }) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(map_err(err)),
    }
}

/// 写入 meta 值（供索引引擎维护重建标记等）。
pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES(?1, ?2)",
        [key, value],
    )
    .map_err(map_err)?;
    Ok(())
}

/// 删除 meta 键（不存在时无副作用）。
pub fn clear_meta(conn: &Connection, key: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM meta WHERE key = ?1", [key])
        .map_err(map_err)?;
    Ok(())
}

/// 表是否存在。
pub fn table_exists(conn: &Connection, name: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(map_err)
}

fn map_err(err: rusqlite::Error) -> AppError {
    tracing::warn!(error = %err, "索引库操作失败");
    AppError::db("访问索引库")
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
