//! 搜索域的**索引侧**查询（PRD §5.3.4 的 search_fulltext / search_quick_open）。
//!
//! 分层：本模块只出**行**（路径/名称/时间/相关度）。片段必须读原文后再定位
//! （FTS5 的 snippet() 作用于分词后的 plain_text，会带空格、与原文不逐字一致，
//! 见 kp_domain::search_hits 的模块注释），故片段生成在命令层。
//!
//! **高级语法过滤条件在这里落成 SQL 谓词**（FR-SEARCH-20~22）：path: → rel_path 前缀、
//! file: → name 子串、tag: → file_tag/tag 关联（含层级前缀）。它们**不进 MATCH**，
//! 理由见 kp_domain::search_syntax 顶部。
//!
//! **排序方向只来自本模块的枚举**：ORDER BY 片段不接受任何来自前端的字符串，
//! 从结构上消除 SQL 注入面（而不是靠转义）。过滤条件的取值一律走**绑定参数**。

use kp_domain::error::AppError;
use kp_domain::search::ParsedQuery;
use rusqlite::{params_from_iter, ToSql};

use std::sync::{Arc, Mutex, OnceLock};

use crate::storage::pool::DbPool;
use crate::storage::search_sql::{build_spec, QuerySpec};

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
    ///
    /// `has_score` 为 false 表示这次查询没有 MATCH（纯过滤查询），此时**没有 bm25 可用**：
    /// 相关度退化为「修改时间倒序」，其余三种排序（FR-SEARCH-06）照常生效 ——
    /// 早期版本在纯过滤查询里**一律**按修改时间排，把用户选的「按路径/按文件名」静默吃掉了。
    pub(super) fn order_by(self, has_score: bool) -> &'static str {
        match (self, has_score) {
            (Self::Relevance, true) => "score ASC, f.rel_path ASC",
            (Self::Relevance, false) | (Self::Mtime, _) => "f.mtime_ms DESC, f.rel_path ASC",
            (Self::Name, _) => "f.name COLLATE NOCASE ASC, f.rel_path ASC",
            (Self::Path, _) => "f.rel_path ASC",
        }
    }
}

/// 一条全文命中（不含片段）。
#[derive(Debug, Clone, PartialEq)]
pub struct FtsHit {
    pub rel_path: String,
    pub name: String,
    pub mtime_ms: i64,
    /// bm25 相关度（负值，越小越相关）；纯过滤查询没有 bm25，固定为 0。
    pub score: f64,
}

/// 执行主查询；`page` 为 None 时不加 LIMIT（供 `line:` 的全候选扫描用）。
fn fetch(
    pool: &DbPool,
    spec: QuerySpec,
    page: Option<(i64, i64)>,
) -> Result<Vec<FtsHit>, AppError> {
    pool.with_reader(move |conn| {
        let tail = if page.is_some() {
            " LIMIT ? OFFSET ?"
        } else {
            ""
        };
        let sql = format!(
            "{} {} ORDER BY {}{tail}",
            spec.select, spec.from_where, spec.order_by
        );
        let mut values: Vec<&dyn ToSql> = spec.params.iter().map(|p| p as &dyn ToSql).collect();
        let (limit, offset) = page.unwrap_or((0, 0));
        if page.is_some() {
            values.push(&limit);
            values.push(&offset);
        }
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::db_with("准备全文搜索", &e))?;
        let rows = stmt
            .query_map(params_from_iter(values), |r| {
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
        Ok(hits)
    })
}

/// 分页全文搜索：返回（本页命中, 命中总数）。
///
/// total 与 items 走**同一 spec**（同一 MATCH 表达式 + 同一过滤谓词），
/// 否则会出现「总数与结果对不上」，分页随即漏项或重复（IPC-05）。
pub fn fulltext_page(
    pool: &DbPool,
    parsed: &ParsedQuery,
    sort: SortOrder,
    limit: i64,
    offset: i64,
) -> Result<(Vec<FtsHit>, i64), AppError> {
    let spec = build_spec(parsed, sort);
    let count_sql = format!("SELECT count(*) {}", spec.from_where);
    let count_params: Vec<String> = spec.params.clone();
    let hits = fetch(pool, spec, Some((limit, offset)))?;
    let total = pool.with_reader(move |conn| {
        conn.query_row(&count_sql, params_from_iter(count_params.iter()), |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|e| AppError::db_with("统计全文搜索总数", &e))
    })?;
    Ok((hits, total))
}

/// 取**全部**候选行（不分页）。
///
/// 只有 `line:(...)` 需要它：同一行共现无法在 SQL 里表达（索引里没有换行信息），
/// 必须拿到完整候选集合才能读原文过滤，并据此算出**过滤后**的 total。
pub fn fulltext_scan(
    pool: &DbPool,
    parsed: &ParsedQuery,
    sort: SortOrder,
) -> Result<Vec<FtsHit>, AppError> {
    let spec = build_spec(parsed, sort);
    fetch(pool, spec, None)
}

/// 快速打开的候选（FR-SEARCH-40：**只按文件名/路径，不搜正文**）。
///
/// 只取 kind = 'note'：附件与其它文件不是「笔记」，不该出现在快速打开里。
/// 路径升序返回，让打分器在**同分**时的顺序可复现（rank 是稳定排序）。
/// **候选缓存**（NFR-PERF-05 优化，2026-10-28）。
///
/// 为什么放在存储层：候选来自索引库，唯一正确性依据是「库里的 file 行」。
/// 缓存以**库内容指纹**为键（count / max(mtime) / sum(mtime) / 路径总长 / 文件名总长 / vault_root）：
/// 任何插入/删除/改名/重新索引导致这些量变化都会**自动失效**，不依赖调用方记得清缓存。
/// 放在进程内全局（按指纹区分 Vault）——同一 Vault 命中，不同 Vault 的 vault_root 不同，不会串数据。
struct CandidateCache {
    fingerprint: (i64, i64, i64, i64, i64, i64, String),
    rows: Arc<Vec<(String, String)>>,
}

static CANDIDATE_CACHE: OnceLock<Mutex<Option<CandidateCache>>> = OnceLock::new();

type Fingerprint = (i64, i64, i64, i64, i64, i64, String);

fn candidates_fingerprint(conn: &rusqlite::Connection) -> Result<Fingerprint, AppError> {
    let (count, max_mtime, sum_mtime, sum_path, sum_name): (i64, i64, i64, i64, i64) = conn
        .query_row(
            "SELECT count(*), coalesce(max(mtime_ms),0), coalesce(sum(mtime_ms),0),              coalesce(sum(length(rel_path)),0), coalesce(sum(length(name)),0)              FROM file WHERE deleted = 0 AND kind = 'note'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(|e| AppError::db_with("快速打开候选指纹", &e))?;
    let root = crate::storage::index::read_meta(conn, "vault_root")?.unwrap_or_default();
    // 代次由全量索引完成时推进：覆盖「改名但总长/mtime 不变」这类指纹碰撞（见 index_engine）。
    let generation: i64 = crate::storage::index::read_meta(conn, "quick_open_generation")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    Ok((
        count, max_mtime, sum_mtime, sum_path, sum_name, generation, root,
    ))
}

pub fn quick_open_candidates(pool: &DbPool) -> Result<Arc<Vec<(String, String)>>, AppError> {
    let fingerprint = pool.with_reader(candidates_fingerprint)?;
    let cache = CANDIDATE_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(hit) = cache.lock().expect("候选缓存锁").as_ref() {
        if hit.fingerprint == fingerprint {
            return Ok(hit.rows.clone());
        }
    }
    let rows = load_quick_open_candidates(pool)?;
    *cache.lock().expect("候选缓存锁") = Some(CandidateCache {
        fingerprint,
        rows: rows.clone(),
    });
    Ok(rows)
}

fn load_quick_open_candidates(pool: &DbPool) -> Result<Arc<Vec<(String, String)>>, AppError> {
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
    .map(Arc::new)
}

#[cfg(test)]
#[path = "search_query_tests.rs"]
mod tests;
