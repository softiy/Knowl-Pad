//! tag 表的**批量读取**：一条 \`SELECT … WHERE norm IN (…) \` 拿回 chunk 内所有已有标签。
//!
//! 单独成文件是为了 CODE-11：批量写的编排（\`index_tag_batch.rs\`）与纯查询分开，
//! 且两者都留在 200 SLOC 的预警线内。

use std::collections::HashMap;

use rusqlite::{params_from_iter, Connection};

use kp_domain::error::AppError;

/// 一条 IN (…) 查询一次最多绑定的参数个数（保守取 SQLite 老版本的 999 上限）。
const TAG_QUERY_CHUNK: usize = 999;

/// 已在 tag 表里的行（批量 SELECT 的结果）。
pub(crate) struct ExistingTag {
    pub(crate) id: i64,
    pub(crate) depth: i64,
    pub(crate) is_leaf: i64,
}

/// 批量取 tag 行：按 999 个占位符切块，避免撞 SQLite 的绑定变量上限。
pub(crate) fn select_tag_rows(
    conn: &Connection,
    norms: &[&str],
) -> Result<HashMap<String, ExistingTag>, AppError> {
    let mut out = HashMap::with_capacity(norms.len());
    for chunk in norms.chunks(TAG_QUERY_CHUNK) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("SELECT id, norm, depth, is_leaf FROM tag WHERE norm IN ({placeholders})");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::db_with("准备批量读取标签 id", &e))?;
        let mut rows = stmt
            .query_map(params_from_iter(chunk.iter().copied()), |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    ExistingTag {
                        id: r.get(0)?,
                        depth: r.get(2)?,
                        is_leaf: r.get(3)?,
                    },
                ))
            })
            .map_err(|e| AppError::db_with("批量读取标签 id", &e))?;
        for row in rows.by_ref() {
            let (norm, tag) = row.map_err(|e| AppError::db_with("批量读取标签 id", &e))?;
            out.insert(norm, tag);
        }
    }
    Ok(out)
}
