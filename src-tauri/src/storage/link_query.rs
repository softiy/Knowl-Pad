//! 链接查询层（M4 / FR-LINK-10~22）：**只读 SQL**，供命令层组装 IPC 形状。
//!
//! 分工：本层只查索引库（`link`/`file`/`heading` 表）；**上下文片段**（FR-LINK-11）由命令层
//! 读笔记正文后调用 `kp_domain::snippet::extract_snippet` 组装 —— 域层不碰 IO，SQL 只在本层。

use kp_domain::error::AppError;
use rusqlite::Connection;

use super::pool::DbPool;

/// 一条反链（FR-LINK-10/11/15）。
#[derive(Debug, Clone, PartialEq, Eq)]
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub struct BacklinkRow {
    pub src_rel_path: String,
    pub src_name: String,
    pub link_kind: String,
    pub target_ref: String,
    pub alias: Option<String>,
    pub anchor: Option<String>,
    pub line: u32,
    pub col: u32,
}

/// 一条出链（含悬空/歧义状态）。
#[derive(Debug, Clone, PartialEq, Eq)]
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub struct OutgoingRow {
    pub target_ref: String,
    pub status: String,
    pub anchor: Option<String>,
    pub alias: Option<String>,
    pub link_kind: String,
    pub line: u32,
    pub col: u32,
    pub dst_rel_path: Option<String>,
}

/// 悬空链接按目标名分组（FR-LINK-20）。
#[derive(Debug, Clone, PartialEq, Eq)]
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub struct DanglingGroup {
    pub target_ref: String,
    /// 引用处数
    pub ref_count: u32,
    /// 引用它的笔记数
    pub source_count: u32,
    /// 示例来源（最多 3 个，按路径排序，便于 UI 展示）
    pub sample_sources: Vec<String>,
}

/// 歧义链接（FR-LINK-22 / AC-LINK-04）。
#[derive(Debug, Clone, PartialEq, Eq)]
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub struct AmbiguousRow {
    pub target_ref: String,
    /// 候选目标的相对路径（按路径排序，确定性）
    pub candidates: Vec<String>,
    /// 引用处数
    pub ref_count: u32,
}

/// 标题（`link_headings` 用；编辑器补全/锚点校验展示）。
#[derive(Debug, Clone, PartialEq, Eq)]
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub struct HeadingRow {
    pub level: u32,
    pub text: String,
    pub anchor: String,
    pub line: u32,
}

fn note_id(conn: &Connection, rel_path: &str) -> Result<Option<i64>, AppError> {
    conn.query_row(
        "SELECT id FROM file WHERE rel_path = ?1",
        rusqlite::params![rel_path],
        |r| r.get::<_, i64>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(AppError::db("查询文件失败")),
    })
}

/// 指向 `dst_rel_path` 的**已解析**反链（AC-LINK-01 的计数口径就是本函数的行数）。
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub fn backlinks_for(pool: &DbPool, dst_rel_path: &str) -> Result<Vec<BacklinkRow>, AppError> {
    pool.with_reader(|conn| {
        let Some(dst) = note_id(conn, dst_rel_path)? else {
            return Ok(Vec::new());
        };
        let mut stmt = conn
            .prepare(
                "SELECT f.rel_path, f.name, l.link_kind, l.target_ref, l.alias, l.anchor, l.line, l.col \
                 FROM link l JOIN file f ON f.id = l.src_file_id \
                 WHERE l.dst_file_id = ?1 AND l.status = 'resolved' \
                 ORDER BY f.rel_path, l.line, l.col",
            )
            .map_err(|_| AppError::db("准备反链查询失败"))?;
        let rows = stmt
            .query_map(rusqlite::params![dst], |r| {
                Ok(BacklinkRow {
                    src_rel_path: r.get(0)?,
                    src_name: r.get(1)?,
                    link_kind: r.get(2)?,
                    target_ref: r.get(3)?,
                    alias: r.get(4)?,
                    anchor: r.get(5)?,
                    line: r.get::<_, i64>(6)? as u32,
                    col: r.get::<_, i64>(7)? as u32,
                })
            })
            .map_err(|_| AppError::db("执行反链查询失败"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::db("读取反链结果失败"))
    })
}

/// 反链计数（AC-LINK-01 要求面板条数与 SQL 计数一致，这个函数就是"SQL 计数"的单一来源）。
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub fn backlink_count(pool: &DbPool, dst_rel_path: &str) -> Result<u32, AppError> {
    Ok(backlinks_for(pool, dst_rel_path)?.len() as u32)
}

/// `src_rel_path` 的出链（含各状态）。
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
pub fn outgoing_for(pool: &DbPool, src_rel_path: &str) -> Result<Vec<OutgoingRow>, AppError> {
    pool.with_reader(|conn| {
        let Some(src) = note_id(conn, src_rel_path)? else {
            return Ok(Vec::new());
        };
        let mut stmt = conn
            .prepare(
                "SELECT l.target_ref, l.status, l.anchor, l.alias, l.link_kind, l.line, l.col, d.rel_path \
                 FROM link l LEFT JOIN file d ON d.id = l.dst_file_id \
                 WHERE l.src_file_id = ?1 ORDER BY l.line, l.col",
            )
            .map_err(|_| AppError::db("准备出链查询失败"))?;
        let rows = stmt
            .query_map(rusqlite::params![src], |r| {
                Ok(OutgoingRow {
                    target_ref: r.get(0)?,
                    status: r.get(1)?,
                    anchor: r.get(2)?,
                    alias: r.get(3)?,
                    link_kind: r.get(4)?,
                    line: r.get::<_, i64>(5)? as u32,
                    col: r.get::<_, i64>(6)? as u32,
                    dst_rel_path: r.get(7)?,
                })
            })
            .map_err(|_| AppError::db("执行出链查询失败"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::db("读取出链结果失败"))
    })
}

/// 悬空链接按目标名分组（FR-LINK-20：按目标名分组 + 显示引用次数）。
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
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

/// 歧义链接（AC-LINK-04：10 条 `[[note]]` → 2 个候选）。
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
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
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
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

/// 某笔记的全部标题（`link_headings`）。
// M4 命令层（下一批：commands/link.rs）接入前本项暂无调用方 ——
// 按 M3 的处置惯例用**逐项窄豁免 + 理由**（模块级 #![allow] 会被审查判为"名不副实"）。
#[allow(dead_code)] // 移除点：命令层接入的那个 PR
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
