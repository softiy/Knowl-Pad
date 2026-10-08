//! 笔记维度的链接查询（反链 / 出链）与文件 id 解析 —— 从 `link_query.rs` 拆出，
//! 使该文件低于 CODE-11 的 200 行警告线。SQL 仍集中在本层（RS-03）。

use kp_domain::error::AppError;
use rusqlite::Connection;

use super::link_query_types::{BacklinkRow, OutgoingRow};
use super::pool::DbPool;

pub(crate) fn note_id(conn: &Connection, rel_path: &str) -> Result<Option<i64>, AppError> {
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

/// 找出**引用了某个目标词干**的全部来源文件（FR-FILE-21 的候选集）。
///
/// 匹配口径与改写器一致：按**词干、大小写不敏感**，并允许目标写作 folder/名
/// （因此比较的是 target_ref 的整串与带目录前缀两种写法，而不是只比相等）。
pub fn files_referencing(pool: &DbPool, target_stem: &str) -> Result<Vec<String>, AppError> {
    let want = target_stem.to_lowercase();
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT f.rel_path FROM link l
                 JOIN file f ON f.id = l.src_file_id
                 WHERE lower(l.target_ref) = ?1
                    OR lower(l.target_ref) LIKE '%/' || ?1
                 ORDER BY f.rel_path",
            )
            .map_err(|e| AppError::db(&format!("准备引用查询失败：{e}")))?;
        let rows = stmt
            .query_map(rusqlite::params![want], |r| r.get::<_, String>(0))
            .map_err(|e| AppError::db(&format!("查询引用失败：{e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AppError::db(&format!("读取引用行失败：{e}")))?);
        }
        Ok(out)
    })
}

/// 按 `link.id` 取一条链接的来源文件与位置（FR-LINK-22 的 `link_resolve_ambiguous` 用）。
pub fn link_by_id(
    pool: &DbPool,
    link_id: i64,
) -> Result<Option<(String, u32, u32, String)>, AppError> {
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT f.rel_path, l.line, l.col, l.target_ref
             FROM link l JOIN file f ON f.id = l.src_file_id
             WHERE l.id = ?1",
            rusqlite::params![link_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(AppError::db(&format!("按 id 取链接失败：{other}"))),
        })
    })
}
