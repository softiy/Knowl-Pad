//! 全文/过滤查询的 **SQL 组装**（从 storage::search_query 拆出：CODE-11 的 200 SLOC 预警线）。
//!
//! 只负责「把 ParsedQuery 变成 SELECT / FROM / WHERE / ORDER BY + 绑定参数」；
//! 执行与分页在 search_query.rs。拆开的另一个好处是 SQL 组装可以**脱离数据库**单测
//! （见 search_sql_tests.rs：谓词与占位符顺序都能直接断言）。
//!
//! **注入面**：过滤条件一律走绑定参数；LIKE 的通配符由 escape_like 中和；
//! ORDER BY 只来自 SortOrder 枚举，不接受任何来自前端的字符串。

use kp_domain::search::{escape_fts, ParsedQuery};

use super::search_query::SortOrder;

/// 命中列（三条查询路径共用，保证 FtsHit 的字段口径一致）。
const HIT_COLUMNS: &str = "f.rel_path, f.name, f.mtime_ms";

/// LIKE 的通配符中和（配 `ESCAPE '\'` 使用）。
///
/// 不做这一步时 `path:a_b` 会命中 `axb`（下划线在 SQL 里是任意单字符），
/// 用户看到的过滤结果会**多出他没要求的笔记**。
fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 过滤条件 → WHERE 谓词片段与绑定参数（占位符 `?` 按出现顺序绑定）。
fn filter_predicates(parsed: &ParsedQuery) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut params: Vec<String> = Vec::new();
    if let Some(prefix) = &parsed.filters.path_prefix {
        sql.push_str(" AND f.rel_path LIKE ? ESCAPE '\\'");
        params.push(format!("{}/%", escape_like(prefix)));
    }
    if let Some(name) = &parsed.filters.file_name {
        sql.push_str(" AND f.name LIKE ? ESCAPE '\\'");
        params.push(format!("%{}%", escape_like(name)));
    }
    for tag in &parsed.filters.tags {
        // FR-SEARCH-22 的层级前缀匹配：MD-TAG-03 已把 #a/b 登记到 a（等值即可命中祖先），
        // 再显式补一条 a/% 覆盖「只登记了后代」的极端情形。
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM file_tag ft JOIN tag t ON t.id = ft.tag_id \
             WHERE ft.file_id = f.id AND (t.norm = ? OR t.norm LIKE ? ESCAPE '\\'))",
        );
        params.push(tag.clone());
        params.push(format!("{}/%", escape_like(tag)));
    }
    (sql, params)
}

/// 纯过滤查询下的排除项（FR-SEARCH-25）。
///
/// FTS5 的 NOT 是**二元**运算符：没有全文正面词就没有左侧操作数，因此改写成
/// `f.id NOT IN (SELECT rowid FROM note_fts WHERE note_fts MATCH ?)`（每个排除词一条子查询）。
fn exclude_predicates(parsed: &ParsedQuery) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut params: Vec<String> = Vec::new();
    for term in &parsed.excludes {
        sql.push_str(" AND f.id NOT IN (SELECT rowid FROM note_fts WHERE note_fts MATCH ?)");
        params.push(escape_fts(term));
    }
    (sql, params)
}

/// 一条查询的 SQL 三件套 + 绑定参数。
pub(super) struct QuerySpec {
    pub(super) select: String,
    pub(super) from_where: String,
    pub(super) order_by: String,
    pub(super) params: Vec<String>,
}

/// 组装 SQL。三条路径：
/// ① 有全文正面词 → note_fts MATCH + 过滤谓词（bm25 可排序）；
/// ② 只有过滤条件 → 直接列 file 表（没有 bm25，相关度无意义，固定按修改时间倒序）；
/// ③ 什么都没有 → 恒假条件兜底，**绝不**退化成「列出全库」。
pub(super) fn build_spec(parsed: &ParsedQuery, sort: SortOrder) -> QuerySpec {
    let (predicates, filter_params) = filter_predicates(parsed);
    if !parsed.positive_expr.is_empty() {
        let mut params = vec![parsed.match_expr.clone()];
        params.extend(filter_params);
        return QuerySpec {
            select: format!("SELECT {HIT_COLUMNS}, bm25(note_fts) AS score"),
            from_where: format!(
                "FROM note_fts JOIN file f ON f.id = note_fts.rowid \
                 WHERE f.deleted = 0 AND note_fts MATCH ?{predicates}"
            ),
            order_by: sort.order_by(true).to_string(),
            params,
        };
    }
    if parsed.filters.is_empty() {
        return QuerySpec {
            select: format!("SELECT {HIT_COLUMNS}, 0.0 AS score"),
            from_where: "FROM file f WHERE 0".to_string(),
            order_by: "f.rel_path ASC".to_string(),
            params: Vec::new(),
        };
    }
    let (not_in, not_in_params) = exclude_predicates(parsed);
    let mut params = filter_params;
    params.extend(not_in_params);
    QuerySpec {
        select: format!("SELECT {HIT_COLUMNS}, 0.0 AS score"),
        // kind = 'note'：与 MATCH 路径对齐（note_fts 只对笔记建行，M3 已保证）
        from_where: format!(
            "FROM file f WHERE f.deleted = 0 AND f.kind = 'note'{predicates}{not_in}"
        ),
        order_by: sort.order_by(false).to_string(),
        params,
    }
}

#[cfg(test)]
#[path = "search_sql_tests.rs"]
mod tests;
