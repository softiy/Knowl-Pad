//! 索引的**写入侧**：把 [`kp_domain::md_parse::ParsedNote`] 落进派生表（标题 / 块 ID / 链接 / 标签 / 别名）。
//!
//! 从 `index_engine.rs` 拆出，理由与 `index_scan.rs` 同源：CODE-11 的文件级硬上限是 350 SLOC。
//! 边界取"事务内、依赖 file_id 的派生写入"这一整体 —— 拆在别处会把事务语义切碎
//! （这些函数**必须**由调用方在同一个事务里调用：先删旧行、再写新行）。

use std::collections::HashMap;

use rusqlite::{params, params_from_iter, Connection};

use kp_domain::error::AppError;
use kp_domain::md_parse;

use crate::index_sql;

/// 一条 IN (…) 查询一次最多绑定的参数个数（保守取 SQLite 老版本的 999 上限）。
const TAG_QUERY_CHUNK: usize = 999;
/// 一条多行 upsert 一次最多写多少行：每行 4 个参数，240 × 4 = 960 ≤ 999。
const TAG_UPSERT_ROWS: usize = 240;

/// 一个 chunk 内待写的标签。
///
/// 只在本批事务内累积，**不跨 chunk**：这样 display 的"首次出现"仍然按全局文件顺序判定，
/// 而结构字段（depth / is_leaf）的"最后一次胜"也能在同一批内算准。
#[derive(Default)]
pub(crate) struct TagBatch {
    /// 按 norm **首次出现**的顺序排列；多行 upsert 的顺序由此确定，tag.id 的分配顺序不变。
    tags: Vec<PendingTag>,
    /// norm → tags 下标。
    by_norm: HashMap<String, usize>,
    /// 每次标签出现对应一行 file_tag（含层级展开出的祖先）。
    file_tags: Vec<PendingFileTag>,
}

struct PendingTag {
    norm: String,
    /// chunk 内该 norm **首次**出现的原文大小写（display 首次胜）。
    display: String,
    /// 最后一次出现决定的层级 / 是否最末级（结构字段最后一次胜）。
    depth: i64,
    is_leaf: bool,
}

struct PendingFileTag {
    file_id: i64,
    tag: usize,
    line: i64,
    col: i64,
    source: &'static str,
    explicit: bool,
}

/// 已在 tag 表里的行（批量 SELECT 的结果）。
struct ExistingTag {
    id: i64,
    depth: i64,
    is_leaf: i64,
}

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

impl TagBatch {
    /// 只收集，不写库：这是"每 chunk 批量写标签"的前提。
    fn collect(&mut self, file_id: i64, note: &md_parse::ParsedNote) {
        for t in &note.tags {
            let depth = t.name.split('/').count() as i64;
            let index = match self.by_norm.get(&t.norm) {
                Some(&index) => index,
                None => {
                    let index = self.tags.len();
                    self.tags.push(PendingTag {
                        norm: t.norm.clone(),
                        display: t.name.clone(),
                        depth,
                        is_leaf: t.is_leaf,
                    });
                    self.by_norm.insert(t.norm.clone(), index);
                    index
                }
            };
            // 结构字段最后一次胜；display 在首次创建时已固定，不再覆盖。
            let pending = &mut self.tags[index];
            pending.depth = depth;
            pending.is_leaf = t.is_leaf;
            self.file_tags.push(PendingFileTag {
                file_id,
                tag: index,
                line: t.line,
                col: t.col as i64,
                source: t.source.as_str(),
                explicit: t.explicit,
            });
        }
    }

    /// 在当前 chunk 事务内落库：批量查 id → 批量 upsert 新 norm → 批量插 file_tag。
    ///
    /// 原子性不变：调用方在 BEGIN IMMEDIATE 之后、COMMIT 之前调用；任何一步失败都由
    /// 外层事务回滚，不会留下"标签写了、file_tag 没写"的半成品。
    pub(crate) fn flush(&mut self, conn: &Connection) -> Result<(), AppError> {
        use crate::index_profile::{start, stop, Phase};
        if self.file_tags.is_empty() {
            return Ok(());
        }
        let t_all = start();
        let t_select = start();
        let existing = self.select_existing(conn)?;
        stop(t_select, Phase::TagSelect);
        let unseen = self.unseen(&existing);

        let t_upsert = start();
        Self::upsert_unseen(conn, &unseen)?;
        stop(t_upsert, Phase::TagUpsert);

        let t_select = start();
        let unseen_norms: Vec<&str> = unseen.iter().map(|t| t.norm.as_str()).collect();
        let new_rows = Self::select_rows(conn, &unseen_norms)?;
        stop(t_select, Phase::TagSelect);
        let ids = self.ids(&existing, &new_rows)?;

        self.update_seen_structure(conn, &existing)?;
        self.insert_file_tags(conn, &ids)?;
        self.clear();
        stop(t_all, Phase::Tag);
        Ok(())
    }

    /// 一次 SELECT id, norm, depth, is_leaf … WHERE norm IN (…)，最多 999 个占位符。
    fn select_existing(&self, conn: &Connection) -> Result<HashMap<String, ExistingTag>, AppError> {
        let norms: Vec<&str> = self.tags.iter().map(|t| t.norm.as_str()).collect();
        Self::select_rows(conn, &norms)
    }

    /// 只取本批尚未在 tag 表中的 norm（保持首次出现顺序）。
    fn unseen<'a>(&'a self, existing: &HashMap<String, ExistingTag>) -> Vec<&'a PendingTag> {
        self.tags
            .iter()
            .filter(|t| !existing.contains_key(&t.norm))
            .collect()
    }

    /// 批量取 tag 行：按 999 个占位符切块，避免撞 SQLite 的绑定变量上限。
    fn select_rows(
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

    /// 多行 upsert 未见过的 norm；display 取 chunk 内首次出现值，结构字段取最后一次。
    ///
    /// **不用 RETURNING 取回新 id**：2026-10-15 在 3000 篇上做过两轮对照，
    /// 单条 "INSERT … ON CONFLICT … RETURNING id" 让 tag 阶段 4541/4647 → 5787/5792 ms（+25%）。
    /// 现在 upsert 之后只对**新增** norm 补查一次 id，查询次数与批大小无关。
    fn upsert_unseen(conn: &Connection, unseen: &[&PendingTag]) -> Result<(), AppError> {
        for chunk in unseen.chunks(TAG_UPSERT_ROWS) {
            let mut sql =
                String::from("INSERT INTO tag (norm, display, depth, is_leaf, ref_count) VALUES ");
            let mut values: Vec<rusqlite::types::Value> = Vec::with_capacity(chunk.len() * 4);
            for (i, t) in chunk.iter().enumerate() {
                if i > 0 {
                    sql.push(',');
                }
                sql.push_str("(?, ?, ?, ?, 0)");
                values.push(t.norm.clone().into());
                values.push(t.display.clone().into());
                values.push(t.depth.into());
                values.push(if t.is_leaf { 1i64 } else { 0i64 }.into());
            }
            sql.push_str(
                " ON CONFLICT(norm) DO UPDATE SET depth = excluded.depth, is_leaf = excluded.is_leaf",
            );
            conn.execute(&sql, params_from_iter(values))
                .map_err(|e| AppError::db_with("批量写入标签", &e))?;
        }
        Ok(())
    }

    /// 已存在的 tag：只更新发生变化的结构字段（display 保持库里首次值）。
    fn update_seen_structure(
        &self,
        conn: &Connection,
        existing: &HashMap<String, ExistingTag>,
    ) -> Result<(), AppError> {
        use crate::index_profile::{start, stop, Phase};
        let t = start();
        for tag in &self.tags {
            let Some(old) = existing.get(&tag.norm) else {
                continue;
            };
            let leaf = if tag.is_leaf { 1i64 } else { 0i64 };
            if old.depth == tag.depth && old.is_leaf == leaf {
                continue;
            }
            // 复用 TAG_UPSERT_SQL：行已存在，ON CONFLICT 只更新结构字段（display 不覆盖）。
            let mut stmt = conn
                .prepare_cached(index_sql::TAG_UPSERT_SQL)
                .map_err(|e| AppError::db_with("准备更新标签结构", &e))?;
            stmt.execute(params![tag.norm, tag.display, tag.depth, leaf])
                .map_err(|e| AppError::db_with("更新标签结构", &e))?;
        }
        stop(t, Phase::TagUpsert);
        Ok(())
    }

    /// 标签下标 → tag.id（已有行来自第一次 SELECT，新插入行来自 upsert 后的补查）。
    fn ids(
        &self,
        existing: &HashMap<String, ExistingTag>,
        new_rows: &HashMap<String, ExistingTag>,
    ) -> Result<Vec<i64>, AppError> {
        self.tags
            .iter()
            .map(|t| {
                existing
                    .get(&t.norm)
                    .or_else(|| new_rows.get(&t.norm))
                    .map(|r| r.id)
                    .ok_or_else(|| AppError::db("批量写入标签后缺少 id"))
            })
            .collect()
    }

    /// 标签写入的最后一跳：prepared FILE_TAG_INSERT_SQL 循环（每次出现一行）。
    fn insert_file_tags(&self, conn: &Connection, ids: &[i64]) -> Result<(), AppError> {
        use crate::index_profile::{start, stop, Phase};
        let t = start();
        {
            let mut stmt = conn
                .prepare_cached(index_sql::FILE_TAG_INSERT_SQL)
                .map_err(|e| AppError::db_with("准备文件标签关联", &e))?;
            for ft in &self.file_tags {
                stmt.execute(params![
                    ft.file_id,
                    ids[ft.tag],
                    ft.line,
                    ft.col,
                    ft.source,
                    if ft.explicit { 1 } else { 0 }
                ])
                .map_err(|e| AppError::db_with("写入文件标签关联", &e))?;
            }
        }
        stop(t, Phase::TagFileTag);
        Ok(())
    }

    fn clear(&mut self) {
        self.tags.clear();
        self.by_norm.clear();
        self.file_tags.clear();
    }
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
