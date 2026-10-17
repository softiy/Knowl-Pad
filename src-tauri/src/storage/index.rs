//! 索引库 index.db：DDL、签名比对与「丢弃重建」（PRD §3.2 / §3.6 MIG-01；技术方案 §4.4 / §4.6）。
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

/// 解析器版本：**PRD §3.1 的任一 MD-* 解析规则（wikilink / tag / heading / frontmatter / block_id）
/// 语义一变，就必须递增它** —— 它是 `IndexSignature` 的一部分，递增即让既有索引失效并重建
/// （技术方案 §4.6 的版本维护纪律；PRD §5.3.3 的 "parser_version" 固定写入 meta）。
///
/// 为什么不能只靠"记得改"：审计 M9 指出，规则改了而这里没动，索引会**静默停留在旧解析结果**上，
/// 而界面上一切正常。所以配了 `parser_source_guard`（见 storage/index_tests.rs）：
/// 它把解析器源码的指纹与本文件记录的基准比对，**源码变了而版本没变就直接红**。
///
/// **2026-10-17（M5 批次 4）：0 → 1。** 升版的**目的**是让"既有索引"与"当前解析器"对齐：
/// 版本号是 `IndexSignature` 的一部分，因此任何 `parser_version ≠ 1` 的索引库在**打开时**就会被
/// `ensure_index_db` 判为过期（`Inspect::Stale`）并**全量重建**（丢弃重建，技术方案 §4.4）——
/// 这正是盖住审计 M9 缺口的那一步：在此之前它恒为 0，解析规则变了索引也不会失效。
/// 判据：`index_tests::index_built_by_older_parser_version_is_rebuilt`（构造 `parser_version = 0` 的
/// **真实签名**，断言被判为 `Rebuilt` 且原因里点名 `parser_version`）。
pub const PARSER_VERSION: u32 = 1;

/// 解析器源码指纹基准 + 与之配对的版本号（由 `parser_source_guard` 用例校验）。
///
/// 维护方式（只有两种合法操作）：
///   · 只改了注释/格式，规则语义没变 → 更新下面的 `PARSER_SOURCE_HASH`（保持 `PARSER_VERSION` 不变）；
///   · 改了任一解析规则的语义 → **两个都要改**：先 +1 `PARSER_VERSION`，再更新 `PARSER_SOURCE_HASH`。
///
/// 当前基准（`cf6b45f7c820e93b`）对应 `PARSER_VERSION = 1`（2026-10-17 升版时的解析器源码）。
// 只在 parser_source_guard 用例里消费（生产路径不需要读它；它是"变更检测"的基准）。
// 不用 #[allow] 就只能靠"假装用一下"，那会让代码说谎 —— 这里按 CODE-10 明确写出理由。
// 2026-10-17（M5 批次 4）：解析器新增**协作式中止**（`parse_with_guard` + 检查点，SEC-03 / DEBT-32）。
// 它**不改解析规则语义**（无预算路径 `parse()` 逐字段与改动前相同，用例 `parse_without_guard_is_unchanged` 钉住）；
// 同一批次把 `PARSER_VERSION` 由 0 提到 1（盖住审计 M9 的缺口），故本基准与版本号 1 配对。
#[allow(dead_code)]
pub const PARSER_SOURCE_HASH: &str = "cf6b45f7c820e93b";
/// 与 `PARSER_SOURCE_HASH` **配对冻结**的版本号。
///
/// 必须是**字面量**、不能写 `= PARSER_VERSION` —— 那样"版本是否变过"的条件会恒假，
/// 护栏就永远拦住任何改动（我第一版就是这么写的，写完立刻发现）。
#[allow(dead_code)]
pub const PARSER_SOURCE_HASH_VERSION: u32 = 1;

/// 分词器版本：M3 引入 jieba-rs 后填写其 crate 版本。
pub const TOKENIZER_VERSION: &str = kp_domain::tokenize::TOKENIZER_VERSION;

/// 词典指纹：M3 由 build.rs 对内嵌词典计算 SHA-256 前 16 位（技术方案 §4.6）。
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
///
/// 生产路径用 [`open_with_outcome`]（它把重建判定带给调用方）；本函数保留给测试与
/// 不需要判定的调用方。
#[allow(dead_code)] // 生产路径用 open_with_outcome（见上）；测试大量使用本函数
pub fn open(vault_root: &Path) -> Result<DbPool, AppError> {
    open_with_outcome(vault_root).map(|(pool, _)| pool)
}

/// 与 `open` 相同，但把**重建判定**一并返回给调用方。
/// M3 复核 M3-8 需要它：`IndexStatus.ready` 此前恒为 true，而"meta 里有没有记录"
/// 不能作为判据（`bootstrap` 开库时无条件写 meta）。真正可信的判据是存储层自己的判定：
pub fn open_with_outcome(
    vault_root: &Path,
) -> Result<(DbPool, super::index_rebuild::RebuildOutcome), AppError> {
    let db_path = vault_root.join(INDEX_DB_REL);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let expected = IndexSignature::current(vault_root);

    // §4.4：先做**文件级**的「丢弃重建 + 原子切换」，再打开连接池。
    // 旧实现是开池后就地 DROP TABLE，那样会先销毁旧索引——FR-SIG-02（重建期旧索引只读可用）
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

    let pool = DbPool::open_with(&db_path, move |conn| bootstrap(conn, &expected))?;
    Ok((pool, outcome))
}

/// 连接池就绪后的 schema 兜底（幂等）：建表 + 写 meta。
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
