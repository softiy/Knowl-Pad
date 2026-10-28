//! 索引的**写入侧**：把 [`kp_domain::md_parse::ParsedNote`] 落进派生表（标题 / 块 ID / 链接 / 标签 / 别名）。
//!
//! 从 `index_engine.rs` 拆出，理由与 `index_scan.rs` 同源：CODE-11 的文件级硬上限是 350 SLOC。
//! 边界取"事务内、依赖 file_id 的派生写入"这一整体 —— 拆在别处会把事务语义切碎
//! （这些函数**必须**由调用方在同一个事务里调用：先删旧行、再写新行）。
//!
//! 标签的 chunk 批量写已拆到 `index_tag_batch.rs`（CODE-11 的 200 行预警线）；
//! 本文件只保留按文件即时写的派生表，以及标签收集的调用点。

use rusqlite::{params, Connection};

use kp_domain::error::AppError;
use kp_domain::md_parse;

use crate::index_sql;
use crate::index_tag_batch::TagBatch;

/// 把单文件解析结果写入派生表（标题 / 块 ID / 链接 / 标签 / 别名）。
///
/// 标签只**收集**到 `tags`，由调用方在本 chunk 结束时统一 `flush` —— 见 [`TagBatch::flush`]。
/// 其余派生表仍按文件即时写入；所有写入都在调用方开启的同一个事务里。
pub(crate) fn insert_note_derived(
    conn: &Connection,
    file_id: i64,
    note: &md_parse::ParsedNote,
    tags: &mut TagBatch,
) -> Result<(), AppError> {
    use crate::index_profile::{start, stop, Phase};
    let t = start();
    for (i, h) in note.headings.iter().enumerate() {
        let mut stmt = conn
            .prepare_cached(index_sql::HEADING_INSERT_SQL)
            .map_err(|e| AppError::db_with("准备写入标题", &e))?;
        stmt.execute(params![
            file_id,
            h.level as i64,
            h.text,
            h.text,
            h.line as i64,
            i as i64
        ])
        .map_err(|e| AppError::db_with("写入标题", &e))?;
    }
    stop(t, Phase::Heading);
    let t = start();
    for b in &note.block_ids {
        let mut stmt = conn
            .prepare_cached(index_sql::BLOCK_ID_INSERT_SQL)
            .map_err(|e| AppError::db_with("准备写入块 ID", &e))?;
        stmt.execute(params![
            file_id,
            b.id,
            b.start_line as i64,
            b.end_line as i64
        ])
        .map_err(|e| AppError::db_with("写入块 ID", &e))?;
    }
    stop(t, Phase::BlockId);
    insert_links(conn, file_id, note)?;
    let t = start();
    tags.collect(file_id, note);
    stop(t, Phase::Tag);
    let t = start();
    if let Some(fm) = note.frontmatter.as_ref() {
        for alias in &fm.aliases {
            let mut stmt = conn
                .prepare_cached(index_sql::ALIAS_INSERT_SQL)
                .map_err(|e| AppError::db_with("准备写入别名", &e))?;
            stmt.execute(params![file_id, alias])
                .map_err(|e| AppError::db_with("写入别名", &e))?;
        }
    }
    stop(t, Phase::Alias);
    Ok(())
}

/// link 表：每条链接一行。SQL 与语义未变，只是走预编译语句缓存（见 index_sql）。
fn insert_links(
    conn: &Connection,
    file_id: i64,
    note: &md_parse::ParsedNote,
) -> Result<(), AppError> {
    use crate::index_profile::{start, stop, Phase};
    let t = start();
    for l in &note.links {
        let kind = match l.kind {
            md_parse::LinkKind::Embed => "embed",
            md_parse::LinkKind::Link => "link",
        };
        let mut stmt = conn
            .prepare_cached(index_sql::LINK_INSERT_SQL)
            .map_err(|e| AppError::db_with("准备写入链接", &e))?;
        stmt.execute(params![
            file_id,
            l.target,
            l.anchor,
            l.alias,
            kind,
            l.line as i64,
            l.col as i64
        ])
        .map_err(|e| AppError::db_with("写入链接", &e))?;
    }
    stop(t, Phase::Link);
    Ok(())
}
