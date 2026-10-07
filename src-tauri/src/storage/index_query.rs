//! 从索引库读文件树（M3 PR-6 / DEBT-15）。
//!
//! **口径（写入台账）**：默认视图（`include_hidden = false`）走**索引库**，避免每次展开都递归 readdir；
//! `include_hidden = true` 仍走**直读**（`file_tree::list_dir`）—— 因为索引按 `IGNORED_DIRS` 跳过了 `.obsidian`/`.git` 等，
//! 而「显示隐藏项」这个开关的语义应当是「让我看见它们」，不能因为索引没收录就说它们不存在。
//!
//! 目录节点由**路径前缀**推导（`file` 表只存文件），与直读路径的 `has_children` 语义对齐。

use kp_domain::error::AppError;
use kp_domain::file_tree::{classify, FileEntry, FileKind};

use crate::storage::pool::DbPool;

/// 索引库里是否有可用数据（用于决定走索引还是回退直读）。
pub fn has_rows(pool: &DbPool) -> bool {
    pool.with_reader(|conn| {
        conn.query_row("SELECT count(*) FROM file WHERE deleted = 0", [], |r| {
            r.get::<_, i64>(0)
        })
        .map(|n| n > 0)
        .map_err(|_| AppError::db("探测索引库"))
    })
    .unwrap_or(false)
}

/// 列出某个目录下的直接子项（由索引库推导）。`parent_rel` 为空表示 Vault 根。
pub fn list_children(pool: &DbPool, parent_rel: &str) -> Result<Vec<FileEntry>, AppError> {
    let prefix = if parent_rel.is_empty() {
        String::new()
    } else {
        format!("{}/", parent_rel.trim_end_matches('/'))
    };
    let prefix_chars = prefix.chars().count() as i64;
    let rows: Vec<(String, String, Option<i64>, Option<i64>)> = pool.with_reader(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT rel_path, kind, size_bytes, mtime_ms FROM file \
                  WHERE deleted = 0 AND (?1 = 0 OR substr(rel_path, 1, ?1) = ?2) \
                  ORDER BY rel_path",
            )
            .map_err(|_| AppError::db("准备文件树查询"))?;
        let rows = stmt
            .query_map(rusqlite::params![prefix_chars, prefix], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|_| AppError::db("查询文件树"))?;
        Ok(rows.filter_map(Result::ok).collect())
    })?;

    let mut entries: Vec<FileEntry> = Vec::new();
    for (rel_path, kind, size_bytes, mtime_ms) in rows {
        let rest = rel_path
            .strip_prefix(prefix.as_str())
            .unwrap_or(rel_path.as_str());
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            // 更深一层 → 这是一个目录节点（同名只出现一次）
            Some((dir, _)) => {
                if entries.iter().any(|e| e.is_dir && e.name == dir) {
                    continue;
                }
                entries.push(FileEntry {
                    rel_path: format!("{prefix}{dir}"),
                    name: dir.to_string(),
                    is_dir: true,
                    kind: FileKind::Other,
                    size_bytes: None,
                    mtime_ms: None,
                    has_children: Some(true),
                });
            }
            None => {
                let name = rest.to_string();
                let ext = rel_path.rsplit('.').next().unwrap_or("");
                entries.push(FileEntry {
                    rel_path: rel_path.clone(),
                    name,
                    is_dir: false,
                    kind: if kind.is_empty() {
                        classify(ext)
                    } else {
                        kind_from_db(&kind, ext)
                    },
                    size_bytes: size_bytes.map(|v| v.max(0) as u64),
                    mtime_ms,
                    has_children: None,
                });
            }
        }
    }
    // 目录在前、其余按名称（与直读路径的排序保持一致）
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// 索引里的 kind 字符串 → 领域枚举（未知值退回按扩展名分类）。
fn kind_from_db(kind: &str, ext: &str) -> FileKind {
    match kind {
        "note" => FileKind::Note,
        "attachment" => FileKind::Attachment,
        _ => classify(ext),
    }
}
