#![allow(dead_code)]
//! 链接裁决与标签计数重算（勘误 D-20：裁决属 **M3 索引阶段**，不是 M4）。
//!
//! 三步候选（技术方案 §5.3.4）：
//! 1. **stem 匹配**：`lower(stem) = lower(target_ref)`；
//! 2. **路径匹配**：`lower(rel_path) = lower(target_ref)`（含/不含 `.md` 两种写法）；
//! 3. **别名匹配**：`file_alias.alias` 大小写不敏感相等（依赖 `idx_alias_lower`，DEBT-07 已关闭）；
//!
//! 判定：候选恰好 1 个 → `resolved` 并写入 `dst_file_id`；0 个 → `dangling`；≥2 个 → `ambiguous`。
//! **本切片不自动消歧**（MD-WL-04 的自动消解规则留待补全，登记在计划 §5.2）——
//! 宁可标 ambiguous 让人看见，也不猜错一个链接。

use rusqlite::Connection;

use kp_domain::error::AppError;

/// 裁决结果统计。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolveOutcome {
    pub resolved: usize,
    pub dangling: usize,
    pub ambiguous: usize,
}

/// 对全部链接行重新裁决（**幂等**：可重复调用）。
pub fn resolve_links(conn: &Connection) -> Result<ResolveOutcome, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, target_ref FROM link")
        .map_err(|_| AppError::db("读取待裁决链接"))?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| AppError::db("读取待裁决链接"))?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut out = ResolveOutcome::default();
    for (link_id, target) in rows {
        let target = target.trim();
        if target.is_empty() {
            continue;
        }
        let with_md = if target.to_lowercase().ends_with(".md") {
            target.to_string()
        } else {
            format!("{target}.md")
        };
        // 三步候选（合并为一次查询，保留判定顺序无关性）
        let mut stmt = conn
            .prepare(
                "SELECT id FROM file WHERE deleted = 0 AND (\
                   lower(stem) = lower(?1) OR lower(rel_path) = lower(?2) OR lower(rel_path) = lower(?1)\
                 ) UNION \
                 SELECT fa.file_id FROM file_alias fa JOIN file f ON f.id = fa.file_id \
                  WHERE f.deleted = 0 AND lower(fa.alias) = lower(?1)",
            )
            .map_err(|_| AppError::db("准备候选查询"))?;
        let candidates: Vec<i64> = stmt
            .query_map(rusqlite::params![target, with_md], |r| r.get(0))
            .map_err(|_| AppError::db("查询链接候选"))?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);
        let (status, dst) = match candidates.len() {
            1 => ("resolved", Some(candidates[0])),
            0 => ("dangling", None),
            _ => ("ambiguous", None),
        };
        conn.execute(
            "UPDATE link SET status = ?1, dst_file_id = ?2 WHERE id = ?3",
            rusqlite::params![status, dst, link_id],
        )
        .map_err(|_| AppError::db("写回裁决结果"))?;
        match status {
            "resolved" => out.resolved += 1,
            "ambiguous" => out.ambiguous += 1,
            _ => out.dangling += 1,
        }
    }
    Ok(out)
}

/// 重算标签 `ref_count`（= 引用该标签的文件-标签行数）。
pub fn recount_tags(conn: &Connection) -> Result<usize, AppError> {
    let n = conn
        .execute(
            "UPDATE tag SET ref_count = (SELECT count(*) FROM file_tag WHERE file_tag.tag_id = tag.id)",
            [],
        )
        .map_err(|_| AppError::db("重算标签引用计数"))?;
    Ok(n)
}
