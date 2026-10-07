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
pub const TOKENIZER_VERSION: &str = kp_domain::tokenize::TOKENIZER_VERSION;

/// 词典指纹：M3 由 build.rs 对内嵌词典计算 SHA-256 前 16 位（技术方案 §4.6）。
/// 词典哈希（进索引签名）。
///
/// jieba-rs 的词典**编译期内嵌**于二进制、运行时没有可哈希的词典文件，因此这里用
/// **版本 + 特性 + 内置词典标识**的 FNV-1a 64 十六进制作为「变化检测」指纹：
/// 任何一项变化都会让签名变化 → 触发全量重建（正确性优先于性能的刻意取舍）。
/// 复现方式：FNV-1a64("jieba-rs|<version>|default-dict")。
pub const TOKENIZER_DICT_HASH: &str = "5d5182e208a4a8a0";

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

    // §4.4：先做**文件级**的「丢弃重建 + 原子切换」，再打开连接池。
    // 旧实现是开池后就地 DROP TABLE，那样会先销毁旧索引——FR-SIG-02（重建期旧索引只读可用）
    // 在其上无法实现。
    let outcome = super::index_rebuild::ensure_index_db(&db_path, &expected)?;
    match &outcome {
        super::index_rebuild::RebuildOutcome::Unchanged => {
            tracing::debug!("索引签名一致，沿用既有索引")
        }
        super::index_rebuild::RebuildOutcome::Fresh => tracing::info!("索引库首次创建"),
        super::index_rebuild::RebuildOutcome::Rebuilt { reason } => tracing::info!(
            reason = %reason,
            "索引库已丢弃重建（M3 将据此发出 kp://index/rebuild-required）"
        ),
    }

    DbPool::open_with(&db_path, move |conn| bootstrap(conn, &expected))
}

/// 连接池就绪后的 schema 兜底（幂等）：建表 + 写 meta。
///
/// 重建判定已在 ensure_index_db 中按 §4.4 完成（文件级），此处不再比较签名。
fn bootstrap(conn: &mut Connection, expected: &IndexSignature) -> Result<(), AppError> {
    create_schema(conn)?;
    write_meta(conn, expected)?;
    Ok(())
}

/// 从 meta 还原上次写入的签名（用于精确的差异原因）。
pub(crate) fn stored_signature(conn: &Connection) -> Result<Option<IndexSignature>, AppError> {
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
pub(crate) fn create_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(DDL_V1).map_err(map_err)
}

/// 写入签名与元信息（schema_version / index_signature / vault_root / parser_version / built_at）。
pub(crate) fn write_meta(conn: &Connection, signature: &IndexSignature) -> Result<(), AppError> {
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
