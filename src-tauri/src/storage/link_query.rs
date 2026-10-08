//! 链接查询层（M4 / FR-LINK-10~22）：**只读 SQL**，供命令层组装 IPC 形状。
//! 分工：本层只查索引库（`link`/`file`/`heading` 表）；**上下文片段**（FR-LINK-11）由命令层

use kp_domain::error::AppError;

use super::pool::DbPool;

// 行结构拆到 link_query_types，这里 re-export 以保持调用方与测试不变。
use super::link_query_notes::note_id;
#[allow(unused_imports)] // 供测试与命令层从 link_query 取用（尚未接入）
pub use super::link_query_notes::{backlinks_for, outgoing_for};
#[allow(unused_imports)] // 同上
pub use super::link_query_types::{
    AmbiguousRow, BacklinkRow, DanglingGroup, HeadingRow, OutgoingRow,
};

/// 反链计数（AC-LINK-01 要求面板条数与 SQL 计数一致，这个函数就是"SQL 计数"的单一来源）。
#[allow(dead_code)] // 命令层接入前暂无调用方
pub fn backlink_count(pool: &DbPool, dst_rel_path: &str) -> Result<u32, AppError> {
    Ok(backlinks_for(pool, dst_rel_path)?.len() as u32)
}

#[allow(dead_code)] // 命令层接入前暂无调用方（移除点：commands/link.rs 落地时）
pub fn dangling_groups(pool: &DbPool) -> Result<Vec<DanglingGroup>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT l.target_ref, f.rel_path FROM link l JOIN file f ON f.id = l.src_file_id \
                 WHERE l.status = 'dangling' ORDER BY l.target_ref, f.rel_path",
            )
            .map_err(|_| AppError::db("准备悬空查询失败"))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|_| AppError::db("执行悬空查询失败"))?;
        let mut groups: Vec<DanglingGroup> = Vec::new();
        let mut sources: Vec<String> = Vec::new();
        for row in rows {
            let (target_ref, src) = row.map_err(|_| AppError::db("读取悬空结果失败"))?;
            match groups.last_mut() {
                Some(g) if g.target_ref == target_ref => {
                    g.ref_count += 1;
                    if !sources.contains(&src) {
                        sources.push(src.clone());
                        g.source_count += 1;
                        if g.sample_sources.len() < 3 {
                            g.sample_sources.push(src);
                        }
                    }
                }
                _ => {
                    groups.push(DanglingGroup {
                        target_ref,
                        ref_count: 1,
                        source_count: 1,
                        sample_sources: vec![src.clone()],
                    });
                    sources = vec![src];
                }
            }
        }
        Ok(groups)
    })
}

#[allow(dead_code)] // 命令层接入前暂无调用方（移除点：commands/link.rs 落地时）
pub fn ambiguous_rows(pool: &DbPool) -> Result<Vec<AmbiguousRow>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT target_ref, COUNT(*) FROM link WHERE status = 'ambiguous' \
                 GROUP BY target_ref ORDER BY target_ref",
            )
            .map_err(|_| AppError::db("准备歧义查询失败"))?;
        let groups: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| AppError::db("执行歧义查询失败"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::db("读取歧义结果失败"))?;
        let mut out = Vec::new();
        for (target_ref, ref_count) in groups {
            // 候选 = 与 target_ref 的最后一段（去 .md）大小写不敏感同名的笔记（FR-LINK-02 ②）
            let stem = target_ref
                .rsplit('/')
                .next()
                .unwrap_or(&target_ref)
                .trim_end_matches(".md");
            let mut cstmt = conn
                .prepare(
                    "SELECT rel_path FROM file WHERE kind = 'note' AND lower(stem) = lower(?1) \
                     ORDER BY rel_path",
                )
                .map_err(|_| AppError::db("准备候选查询失败"))?;
            let candidates: Vec<String> = cstmt
                .query_map(rusqlite::params![stem], |r| r.get::<_, String>(0))
                .map_err(|_| AppError::db("执行候选查询失败"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| AppError::db("读取候选结果失败"))?;
            out.push(AmbiguousRow {
                target_ref,
                candidates,
                ref_count: ref_count as u32,
            });
        }
        Ok(out)
    })
}

/// 孤立笔记：**没有任何出链且没有任何入链**（FR-LINK-21）；自链接两侧都不计（FR-LINK-07）。
#[allow(dead_code)] // 命令层接入前暂无调用方（移除点：commands/link.rs 落地时）
pub fn orphan_notes(pool: &DbPool) -> Result<Vec<String>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT f.rel_path FROM file f WHERE f.kind = 'note' \
                 AND NOT EXISTS (SELECT 1 FROM link l WHERE l.src_file_id = f.id \
                                 AND l.status = 'resolved' AND (l.dst_file_id IS NULL OR l.dst_file_id <> f.id)) \
                 AND NOT EXISTS (SELECT 1 FROM link l WHERE l.dst_file_id = f.id \
                                 AND l.status = 'resolved' AND l.src_file_id <> f.id) \
                 ORDER BY f.rel_path",
            )
            .map_err(|_| AppError::db("准备孤立查询失败"))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|_| AppError::db("执行孤立查询失败"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::db("读取孤立结果失败"))
    })
}

#[allow(dead_code)] // 命令层接入前暂无调用方（移除点：commands/link.rs 落地时）
pub fn headings_of(pool: &DbPool, rel_path: &str) -> Result<Vec<HeadingRow>, AppError> {
    pool.with_reader(|conn| {
        let Some(id) = note_id(conn, rel_path)? else {
            return Ok(Vec::new());
        };
        let mut stmt = conn
            .prepare(
                "SELECT level, text, anchor, line FROM heading WHERE file_id = ?1 ORDER BY sort_order",
            )
            .map_err(|_| AppError::db("准备标题查询失败"))?;
        let rows = stmt
            .query_map(rusqlite::params![id], |r| {
                Ok(HeadingRow {
                    level: r.get::<_, i64>(0)? as u32,
                    text: r.get(1)?,
                    anchor: r.get(2)?,
                    line: r.get::<_, i64>(3)? as u32,
                })
            })
            .map_err(|_| AppError::db("执行标题查询失败"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::db("读取标题结果失败"))
    })
}

#[cfg(test)]
#[path = "link_query_tests.rs"]
mod tests;
