//! 搜索域的**索引侧**查询（PRD §5.3.4 的 search_fulltext / search_quick_open）。
//!
//! 分层：本模块只出**行**（路径/名称/时间/相关度）。片段必须读原文后再定位
//! （FTS5 的 snippet() 作用于分词后的 plain_text，会带空格、与原文不逐字一致，
//! 见 kp_domain::search_hits 的模块注释），故片段生成在命令层。
//!
//! **排序方向只来自本模块的枚举**：ORDER BY 片段不接受任何来自前端的字符串，
//! 从结构上消除 SQL 注入面（而不是靠转义）。

use kp_domain::error::AppError;
use rusqlite::params;

use crate::storage::pool::DbPool;

/// 全文搜索排序（FR-SEARCH-06：相关度 / 修改时间 / 文件名 / 路径；默认相关度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Relevance,
    Mtime,
    Name,
    Path,
}

impl SortOrder {
    /// 线上表示（options.sort）；未知返回 None，由命令层决定默认值。
    pub fn parse(wire: &str) -> Option<Self> {
        match wire {
            "relevance" => Some(Self::Relevance),
            "mtime" => Some(Self::Mtime),
            "name" => Some(Self::Name),
            "path" => Some(Self::Path),
            _ => None,
        }
    }

    /// ORDER BY 片段。bm25() 返回**负值**，越小越相关（FTS5 官方口径）；
    /// 一律追加 rel_path 作为**同分决胜键**，否则分页边界的顺序不稳定（同一页可能重复/漏项）。
    fn order_by(self) -> &'static str {
        match self {
            Self::Relevance => "score ASC, f.rel_path ASC",
            Self::Mtime => "f.mtime_ms DESC, f.rel_path ASC",
            Self::Name => "f.name COLLATE NOCASE ASC, f.rel_path ASC",
            Self::Path => "f.rel_path ASC",
        }
    }
}

/// 一条全文命中（不含片段）。
#[derive(Debug, Clone, PartialEq)]
pub struct FtsHit {
    pub rel_path: String,
    pub name: String,
    pub mtime_ms: i64,
    /// bm25 相关度（负值，越小越相关）
    pub score: f64,
}

const FROM_WHERE: &str =
    "FROM note_fts JOIN file f ON f.id = note_fts.rowid WHERE note_fts MATCH ?1 AND f.deleted = 0";

/// 分页全文搜索：返回（本页命中, 命中总数）。total 与 items 走同一表达式，避免"总数与结果对不上"。
pub fn fulltext_page(
    pool: &DbPool,
    expr: &str,
    sort: SortOrder,
    limit: i64,
    offset: i64,
) -> Result<(Vec<FtsHit>, i64), AppError> {
    let expr = expr.to_string();
    pool.with_reader(move |conn| {
        let sql = format!(
            "SELECT f.rel_path, f.name, f.mtime_ms, bm25(note_fts) AS score {FROM_WHERE} \
             ORDER BY {} LIMIT ?2 OFFSET ?3",
            sort.order_by()
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::db_with("准备全文搜索", &e))?;
        let rows = stmt
            .query_map(params![expr, limit, offset], |r| {
                Ok(FtsHit {
                    rel_path: r.get(0)?,
                    name: r.get(1)?,
                    mtime_ms: r.get(2)?,
                    score: r.get(3)?,
                })
            })
            .map_err(|e| AppError::db_with("执行全文搜索", &e))?;
        let mut hits = Vec::new();
        for row in rows {
            hits.push(row.map_err(|e| AppError::db_with("读取全文搜索结果", &e))?);
        }
        let total: i64 = conn
            .query_row(
                &format!("SELECT count(*) {FROM_WHERE}"),
                params![expr],
                |r| r.get(0),
            )
            .map_err(|e| AppError::db_with("统计全文搜索总数", &e))?;
        Ok((hits, total))
    })
}

/// 快速打开的候选（FR-SEARCH-40：**只按文件名/路径，不搜正文**）。
///
/// 只取 kind = 'note'：附件与其它文件不是「笔记」，不该出现在快速打开里。
/// 路径升序返回，让打分器在**同分**时的顺序可复现（rank 是稳定排序）。
pub fn quick_open_candidates(pool: &DbPool) -> Result<Vec<(String, String)>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT rel_path, name FROM file WHERE deleted = 0 AND kind = 'note' \
                 ORDER BY rel_path",
            )
            .map_err(|e| AppError::db_with("准备快速打开候选", &e))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| AppError::db_with("执行快速打开候选查询", &e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AppError::db_with("读取快速打开候选", &e))?);
        }
        Ok(out)
    })
}

#[cfg(test)]
#[path = "search_query_tests.rs"]
mod tests;
