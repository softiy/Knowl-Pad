//! 全量索引的 **chunk 批量标签写**（NFR-PERF-03 第 1 步）。
//!
//! 旧路径对**每次标签出现**做 3 次 SQL（upsert → select id → insert file_tag）。标准 3000 篇
//! 夹具实测有 24.9 万次标签出现，单这一项就是 tag 阶段的主要成本。
//!
//! 本模块在 `BATCH_SIZE` 个文件的 chunk 内先收集，再在**同一个事务**里：
//! 1. 一次 `SELECT id, norm, depth, is_leaf … WHERE norm IN (… ≤999 个占位符)` 拿已有行；
//! 2. 对未见过的 norm 做多行 upsert（display 取 chunk 内首次、结构字段取最后一次）；
//! 3. 只对**新增** norm 补查一次 id；
//! 4. 用 prepared `FILE_TAG_INSERT_SQL` 循环写 `file_tag`。
//!
//! 唯一性 / display 首次胜 / 结构字段最后胜都由上述顺序保证；原子性由调用方的
//! `BEGIN IMMEDIATE … COMMIT` 保证（本模块不自己开事务）。
//!
//! 拆成独立文件是为了 CODE-11：`index_write.rs` 只保留按文件即时写的派生表。

use std::collections::HashMap;

use rusqlite::{params, params_from_iter, Connection};

use kp_domain::error::AppError;
use kp_domain::md_parse;

use crate::index_sql;
use crate::index_tag_lookup::{select_tag_rows, ExistingTag};

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

impl TagBatch {
    /// 只收集，不写库：这是"每 chunk 批量写标签"的前提。
    pub(crate) fn collect(&mut self, file_id: i64, note: &md_parse::ParsedNote) {
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
        let new_rows = select_tag_rows(conn, &unseen_norms)?;
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
        select_tag_rows(conn, &norms)
    }

    /// 只取本批尚未在 tag 表中的 norm（保持首次出现顺序）。
    fn unseen<'a>(&'a self, existing: &HashMap<String, ExistingTag>) -> Vec<&'a PendingTag> {
        self.tags
            .iter()
            .filter(|t| !existing.contains_key(&t.norm))
            .collect()
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
