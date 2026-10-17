//! 标签域的索引库读取（PRD §5.3.6 的 `tag_tree` / `tag_notes`）。
//!
//! 只**读**索引库（`tag` / `file_tag` / `file`）；树的**构建**是纯逻辑，在 `kp_domain::tag_tree`。
//! 这样分层的理由：SQL 归存储层、判定归 domain，命令层只做参数搬运（CMD-01 / RS-01）。

use kp_domain::error::AppError;
use kp_domain::tag_tree::TagRow;

use crate::storage::pool::DbPool;

/// `tag_notes` 的一行（某标签下的一篇笔记）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedNoteRow {
    pub rel_path: String,
    pub name: String,
    pub mtime_ms: i64,
}

/// 取全部标签行（建树用；`norm` 升序 —— 树里同级的顺序就来自这里）。
pub fn load_tag_rows(pool: &DbPool) -> Result<Vec<TagRow>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare("SELECT norm, display, depth, is_leaf, ref_count FROM tag ORDER BY norm")
            .map_err(|e| AppError::db_with("准备读取标签", &e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TagRow {
                    norm: r.get(0)?,
                    display: r.get(1)?,
                    depth: r.get(2)?,
                    is_leaf: r.get::<_, i64>(3)? != 0,
                    ref_count: r.get(4)?,
                })
            })
            .map_err(|e| AppError::db_with("读取标签", &e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::db_with("读取标签行", &e))?;
        Ok(rows)
    })
}

/// 无标签的笔记数（FR-TAG-13 的「未分类」条目）。
///
/// 口径：`file` 里 `deleted = 0 AND kind = 'note'` 且**没有任何** `file_tag` 关联的笔记。
/// 用 `NOT EXISTS` 而不是 `NOT IN`：后者在子查询出现 NULL 时整条谓词变 NULL（经典陷阱）。
pub fn unclassified_count(pool: &DbPool) -> Result<i64, AppError> {
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT count(*) FROM file f WHERE f.deleted = 0 AND f.kind = 'note' \
             AND NOT EXISTS (SELECT 1 FROM file_tag ft WHERE ft.file_id = f.id)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| AppError::db_with("统计未分类笔记", &e))
    })
}

/// 某标签下的笔记（FR-TAG-03）。
///
/// - `tag_norm` 为空串 ⇒ **「未分类」查询**（列出无任何标签的笔记），与 `TagNode.unclassified` 一一对应；
/// - `include_children = false` ⇒ **只含显式写了该标签的笔记**（`file_tag.explicit = 1`）：
///   MD-TAG-03 的祖先登记会给"写了 `#项目/alpha`"的笔记也写一行 `项目`，但那行 `explicit = 0`，
///   并不表示用户把这篇归到了 `项目`。**2026-10-17（批次 4）起该开关真正生效**，此前是 no-op（勘误 **D-40②**）；
/// - `include_children = true` ⇒ 含**全部子孙**（前缀匹配 `norm = ?1 OR norm LIKE ?1 || '/%'`；
///   祖先登记让子孙行自然落入，语义与生效前一致）；
/// - 同一篇里同一标签出现多次只算**一篇**（`GROUP BY f.id`；AC-TAG-03「点击该标签列出全部 3 篇笔记」）。
pub fn notes_of_tag(
    pool: &DbPool,
    tag_norm: &str,
    include_children: bool,
) -> Result<Vec<TaggedNoteRow>, AppError> {
    if tag_norm.is_empty() {
        return pool.with_reader(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT f.rel_path, f.name, f.mtime_ms FROM file f \
                     WHERE f.deleted = 0 AND f.kind = 'note' \
                     AND NOT EXISTS (SELECT 1 FROM file_tag ft WHERE ft.file_id = f.id) \
                     ORDER BY f.rel_path",
                )
                .map_err(|e| AppError::db_with("准备读取未分类笔记", &e))?;
            collect_notes(&mut stmt, [])
        });
    }
    let sql = if include_children {
        "SELECT f.rel_path, f.name, f.mtime_ms FROM file_tag ft \
         JOIN file f ON f.id = ft.file_id JOIN tag t ON t.id = ft.tag_id \
         WHERE f.deleted = 0 AND f.kind = 'note' AND (t.norm = ?1 OR t.norm LIKE ?1 || '/%') \
         GROUP BY f.id ORDER BY f.rel_path"
    } else {
        "SELECT f.rel_path, f.name, f.mtime_ms FROM file_tag ft \
         JOIN file f ON f.id = ft.file_id JOIN tag t ON t.id = ft.tag_id \
         WHERE f.deleted = 0 AND f.kind = 'note' AND t.norm = ?1 AND ft.explicit = 1 \
         GROUP BY f.id ORDER BY f.rel_path"
    };
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::db_with("准备读取标签笔记", &e))?;
        collect_notes(&mut stmt, [tag_norm])
    })
}

/// 把查询结果收成行（两个分支共用，避免复制粘贴出错）。
fn collect_notes<P: rusqlite::Params>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<TaggedNoteRow>, AppError> {
    let rows = stmt
        .query_map(params, |r| {
            Ok(TaggedNoteRow {
                rel_path: r.get(0)?,
                name: r.get(1)?,
                mtime_ms: r.get(2)?,
            })
        })
        .map_err(|e| AppError::db_with("读取标签笔记", &e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::db_with("读取标签笔记行", &e))?;
    Ok(rows)
}

/// 需要改写的**候选文件**（FR-TAG-07）：写了该标签**或其后代**的笔记。
///
/// 为什么含后代：改名 `a` → `x` 时 `#a/b` 要变成 `#x/b`（"含层级子标签的前缀更新"），
/// 所以 `a/b` 所在的文件也必须进计划。
pub fn files_with_tag(pool: &DbPool, tag_norm: &str) -> Result<Vec<String>, AppError> {
    pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT f.rel_path FROM tag t \
                 JOIN file_tag ft ON ft.tag_id = t.id JOIN file f ON f.id = ft.file_id \
                 WHERE f.deleted = 0 AND f.kind = 'note' AND (t.norm = ?1 OR t.norm LIKE ?1 || '/%') \
                 ORDER BY f.rel_path",
            )
            .map_err(|e| AppError::db_with("准备读取候选文件", &e))?;
        let rows = stmt
            .query_map([tag_norm], |r| r.get::<_, String>(0))
            .map_err(|e| AppError::db_with("读取候选文件", &e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::db_with("读取候选文件行", &e))?;
        Ok(rows)
    })
}
