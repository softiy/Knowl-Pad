//! 索引的**写入侧**：把 [`kp_domain::md_parse::ParsedNote`] 落进派生表（标题 / 块 ID / 链接 / 标签 / 别名）。
//!
//! 从 `index_engine.rs` 拆出，理由与 `index_scan.rs` 同源：CODE-11 的文件级硬上限是 350 SLOC。
//! 边界取"事务内、依赖 file_id 的派生写入"这一整体 —— 拆在别处会把事务语义切碎
//! （这些函数**必须**由调用方在同一个事务里调用：先删旧行、再写新行）。

use rusqlite::{params, Connection};

use kp_domain::error::AppError;
use kp_domain::md_parse;

use crate::index_sql;

/// 把解析结果写入派生表（标题 / 块 ID / 链接 / 标签 / 别名）。
///
/// 抽取自 `index_file_mode`：CODE-11 的函数级上限是 100 SLOC，原函数已到 113。
pub(crate) fn insert_note_derived(
    conn: &Connection,
    file_id: i64,
    note: &md_parse::ParsedNote,
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
    insert_tags(conn, file_id, note)?;
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

/// `link` 表：每条链接一行。SQL 与语义未变，只是走预编译语句缓存（见 `index_sql`）。
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

/// `tag` + `file_tag`：每条标签一行。决策留痕（RETURNING 被实测否掉、改用缓存）见下方注释。
fn insert_tags(
    conn: &Connection,
    file_id: i64,
    note: &md_parse::ParsedNote,
) -> Result<(), AppError> {
    use crate::index_profile::{start, stop, Phase};
    // 注意：下面的循环变量历史上就叫 t —— 计时 token 改名以免与它混淆
    let t_tags = start();
    for t in &note.tags {
        let depth = t.name.split('/').count() as i64;
        // 两段式：先 upsert 标签行，再取回它的 id。
        //
        // **RETURNING 变体实测更慢，已回退**（2026-10-15，标准档 3000 篇、两轮对照）：
        // 把两条语句合成「INSERT … ON CONFLICT … DO UPDATE … RETURNING id」后，
        // tag 阶段 4541 / 4647 ms ⟶ **5787 / 5792 ms（+25%）**，而同一实验里 parse / link /
        // resolve 三个阶段逐轮几乎不变（对照组）⇒ 退化可归因于 RETURNING 本身，不是机器变慢。
        // 于是恢复两段式，并加细分计时（upsert / select / file_tag）定位真正的开销。
        // **预编译语句缓存**（rusqlite 的 prepare_cached）：SQL 文本与结果语义**完全不变**，
        // 只是不再每次调用都重新 parse/codegen。依据见细分计时：单条"按唯一索引取 id"的 SELECT
        // 实测 63 µs/条（946 ms ÷ 15000 条），远超已编译语句应有的量级 ⇒ 开销在 prepare 上。
        // prepare_cached 在同一连接上按 SQL 文本复用语句（SQLITE_PREPARE_PERSISTENT），
        // 作用域结束后自动归还（见 rusqlite/src/cache.rs 的 cache_stmt）。
        let t_upsert = start();
        {
            let mut stmt = conn
                .prepare_cached(index_sql::TAG_UPSERT_SQL)
                .map_err(|e| AppError::db_with("准备标签写入", &e))?;
            stmt.execute(params![
                t.norm,
                t.name,
                depth,
                if t.is_leaf { 1 } else { 0 }
            ])
            .map_err(|e| AppError::db_with("写入标签", &e))?;
        }
        stop(t_upsert, Phase::TagUpsert);
        let t_select = start();
        let tag_id: i64 = {
            let mut stmt = conn
                .prepare_cached(index_sql::TAG_SELECT_ID_SQL)
                .map_err(|e| AppError::db_with("准备读取标签 id", &e))?;
            stmt.query_row(params![t.norm], |r| r.get(0))
                .map_err(|e| AppError::db_with("读取标签 id", &e))?
        };
        stop(t_select, Phase::TagSelect);
        let t_filetag = start();
        {
            let mut stmt = conn
                .prepare_cached(index_sql::FILE_TAG_INSERT_SQL)
                .map_err(|e| AppError::db_with("准备文件标签关联", &e))?;
            stmt.execute(params![file_id, tag_id, t.line, t.col as i64])
                .map_err(|e| AppError::db_with("写入文件标签关联", &e))?;
        }
        stop(t_filetag, Phase::TagFileTag);
    }
    stop(t_tags, Phase::Tag);
    Ok(())
}
