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
    // 审计 I4：此前三层 if let Ok 把**真正的 DB 错误**与"这一行还不存在"混为一谈且零日志 ——
    // 于是别名键静默缺失，指向该文件的链接裁决会停在陈旧状态。现在区分两者：
    // QueryReturnedNoRows 是正常情形（新文件/已删除），其余错误留痕并上报。
    let id = match conn.query_row(
        "SELECT id FROM file WHERE rel_path = ?1",
        rusqlite::params![rel],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(id) => Some(id),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(err) => {
            tracing::warn!(error = %err, "采集目标键失败：查文件行出错（链接裁决可能停留陈旧）");
            return Err(AppError::db("采集目标键"));
        }
    };
    if let Some(id) = id {
        match conn.prepare("SELECT alias FROM file_alias WHERE file_id = ?1") {
            Ok(mut stmt) => {
                match stmt.query_map(rusqlite::params![id], |r| r.get::<_, String>(0)) {
                    Ok(rows) => {
                        for alias in rows.flatten() {
                            out.push(alias.to_lowercase());
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "采集目标键失败：读别名出错");
                        return Err(AppError::db("采集目标键"));
                    }
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "采集目标键失败：准备别名查询出错");
                return Err(AppError::db("采集目标键"));
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
                // 审计 I3：配对失败时**不能整条丢弃** —— 那会让旧路径的索引行留下（幽灵条目：
                // 文件树里已经不存在，索引却还在，搜索/反链都会命中它）。配不上就按"删除"处理。
                let usable: Vec<_> = paths.into_iter().filter(|p| !is_ignored(root, p)).collect();
                let mut paired = false;
                if usable.len() == 2 {
                    if let (Some(f), Some(t)) = (rel_of(root, &usable[0]), rel_of(root, &usable[1]))
                    {
                        if seen.insert(format!("r:{f}->{t}")) {
                            out.push(Change::Renamed(f, t));
                        }
                        paired = true;
                    }
                }
                if !paired {
                    tracing::warn!(
                        count = usable.len(),
                        "重命名事件未能配对，按删除处理以避免索引幽灵条目"
                    );
                    for p in usable {
                        if let Some(rel) = rel_of(root, &p) {
                            if seen.insert(format!("d:{rel}")) {
                                out.push(Change::Removed(rel));
                            }
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
