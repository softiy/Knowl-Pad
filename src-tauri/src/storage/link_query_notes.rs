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
