//! 监听的路径工具：忽略规则、路径换算、单文件读取与事件归并。
//!
//! 从 `index_watch` 拆出，既避免单文件超 CODE-11 硬上限，也让「忽略规则与遍历同源」这条约束
//! 集中在一个文件里（改动时只有一处需要同步）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use notify::EventKind;

use kp_domain::error::AppError;

use crate::index_engine::{ScanEntry, IGNORED_DIRS};
use crate::index_watch::Change;

/// 该路径是否应被忽略（与遍历同源）。
pub fn is_ignored(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return true;
    };
    rel.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name.starts_with(".kp-tmp-") || IGNORED_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d))
    })
}

fn rel_of(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// 采集某个相对路径的「目标键」（小写）：路径本身、去 `.md`、文件名 stem，以及**旧行仍存在时**的别名。
/// 裁决时用它找出「指向这个文件的其他链接」（哪怕该文件刚被删除或改名）。
pub(crate) fn collect_keys(
    conn: &rusqlite::Connection,
    rel: &str,
    out: &mut Vec<String>,
) -> Result<(), AppError> {
    let lower = rel.to_lowercase();
    out.push(lower.clone());
    if let Some(stripped) = lower.strip_suffix(".md") {
        out.push(stripped.to_string());
    }
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.rsplit_once('.') {
        Some((stem, _)) => out.push(stem.to_lowercase()),
        None => out.push(name.to_lowercase()),
    }
    if let Ok(id) = conn.query_row(
        "SELECT id FROM file WHERE rel_path = ?1",
        rusqlite::params![rel],
        |r| r.get::<_, i64>(0),
    ) {
        if let Ok(mut stmt) = conn.prepare("SELECT alias FROM file_alias WHERE file_id = ?1") {
            if let Ok(rows) = stmt.query_map(rusqlite::params![id], |r| r.get::<_, String>(0)) {
                for alias in rows.flatten() {
                    out.push(alias.to_lowercase());
                }
            }
        }
    }
    Ok(())
}

/// 一次变更涉及的全部相对路径。
pub(crate) fn rels_of(change: &Change) -> Vec<String> {
    match change {
        Change::Created(r) | Change::Modified(r) | Change::Removed(r) => vec![r.clone()],
        Change::Renamed(from, to) => vec![from.clone(), to.clone()],
    }
}

/// 把 notify 事件归并成变更列表（同路径只保留最后一次，且去掉 create+modify 的重复）。
pub fn merge_events(root: &Path, kinds: Vec<(EventKind, Vec<PathBuf>)>) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (kind, paths) in kinds {
        match kind {
            EventKind::Create(_) => {
                for p in paths {
                    if is_ignored(root, &p) {
                        continue;
                    }
                    if let Some(rel) = rel_of(root, &p) {
                        if seen.insert(format!("c:{rel}")) {
                            out.push(Change::Created(rel));
                        }
                    }
                }
            }
            EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                // 重命名由 debouncer 的 RenameMode 处理；此处按「两个路径」尽力配对
                let mut it = paths.into_iter().filter(|p| !is_ignored(root, p));
                if let (Some(from), Some(to)) = (it.next(), it.next()) {
                    if let (Some(f), Some(t)) = (rel_of(root, &from), rel_of(root, &to)) {
                        if seen.insert(format!("r:{f}->{t}")) {
                            out.push(Change::Renamed(f, t));
                        }
                    }
                }
            }
            EventKind::Modify(_) => {
                for p in paths {
                    if is_ignored(root, &p) {
                        continue;
                    }
                    if let Some(rel) = rel_of(root, &p) {
                        if seen.insert(format!("m:{rel}")) {
                            out.push(Change::Modified(rel));
                        }
                    }
                }
            }
            EventKind::Remove(_) => {
                for p in paths {
                    if is_ignored(root, &p) {
                        continue;
                    }
                    if let Some(rel) = rel_of(root, &p) {
                        if seen.insert(format!("d:{rel}")) {
                            out.push(Change::Removed(rel));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn scan_entry(root: &Path, rel: &str) -> Option<ScanEntry> {
    let abs = root.join(rel);
    let meta = std::fs::metadata(&abs).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some(ScanEntry {
        rel_path: rel.to_string(),
        size_bytes: meta.len(),
        mtime_ms,
    })
}
