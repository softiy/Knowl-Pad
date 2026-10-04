//! 备份与回收站的**存储机制**（FR-TRASH-02/12、NFR-REL-05、AC-FILE-04）。
//!
//! 布局（PRD §3.3 / §4.10）：
//! - 覆盖前备份：`.knowlpad/backup/<时间戳>/<相对路径>`，保留最近 N 次（默认 10，最新一次永不淘汰）；
//! - 回收站：`.knowlpad/trash/<yyyy-MM>/<时间戳>-<序号>-<原名>`，原始相对路径记入
//!   `.knowlpad/trash/manifest.json`（**权威来源**：实体文件 + 清单，不依赖可丢弃的索引库）。

use crate::error::AppError;
use crate::path_guard::PathGuard;
use crate::vault_paths::{BACKUP_DIR, INTERNAL_DIR, TRASH_DIR, TRASH_MANIFEST};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const BACKUP_KEEP_DEFAULT: usize = 10;

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn backup_existing(
    guard: &PathGuard,
    abs: &Path,
    rel: &str,
    stamp: i64,
) -> Result<String, AppError> {
    let dir = format!("{INTERNAL_DIR}/{BACKUP_DIR}/{stamp}");
    let target = guard
        .canonical_root()
        .join(INTERNAL_DIR)
        .join(BACKUP_DIR)
        .join(stamp.to_string())
        .join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(abs, &target)?;
    // 域层不引日志依赖：把备份位置作为事实返回，由命令边界记录（SEC-09：只记路径，不记正文）
    Ok(format!("{dir}/{}", normalize_rel(rel)))
}

/// 备份保留策略（NFR-REL-05）：按时间戳淘汰旧目录，保留最近 `keep` 次；**最新一次永不淘汰**。
pub fn prune_backups(root: &Path, keep: usize) -> Result<usize, AppError> {
    let dir = root.join(INTERNAL_DIR).join(BACKUP_DIR);
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut stamps: Vec<String> = std::fs::read_dir(&dir)?
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    stamps.sort();
    let keep = keep.max(1);
    let mut removed = 0usize;
    while stamps.len() > keep {
        let victim = stamps.remove(0);
        std::fs::remove_dir_all(dir.join(&victim))?;
        removed += 1;
    }
    Ok(removed)
}

/// 追加回收站清单条目（`.knowlpad/trash/manifest.json`，FR-TRASH-02/12）。
///
/// 域层不依赖 serde，故采用**逐行追加**的稳定格式（每行一个对象，整体始终是合法 JSON 数组）：
/// 追加 = 在最后一个 `]` 前插入 `,` + 换行 + 条目。路径已由 `PathGuard` 逐段校验，这里仍做转义。
/// 把条目移入回收站并登记清单，返回回收站内的相对路径。
/// 清单登记失败会**回滚移动**——绝不出现「文件已消失但回收站不知道」（FR-TRASH-12）。
pub fn move_to_trash(
    root: &Path,
    abs: &Path,
    rel_path: &str,
    is_dir: bool,
) -> Result<String, AppError> {
    let stamp = now_ms();
    let month = yyyy_mm(stamp);
    let trash_dir = root.join(INTERNAL_DIR).join(TRASH_DIR).join(&month);
    std::fs::create_dir_all(&trash_dir)?;
    let original_name = abs
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "item".to_string());
    let id = format!(
        "{stamp}-{}-{original_name}",
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let trash_abs = trash_dir.join(&id);
    let trash_rel = format!("{INTERNAL_DIR}/{TRASH_DIR}/{month}/{id}");

    std::fs::rename(abs, &trash_abs)?;
    if let Err(err) = append_trash_manifest(root, &id, rel_path, is_dir, stamp, &trash_rel) {
        let _ = std::fs::rename(&trash_abs, abs);
        return Err(AppError::IoFailure(format!(
            "回收站清单写入失败，已回滚删除（未删除任何文件）：{err}"
        )));
    }
    Ok(trash_rel)
}

fn append_trash_manifest(
    root: &Path,
    id: &str,
    original_rel_path: &str,
    is_dir: bool,
    deleted_at_ms: i64,
    trash_rel_path: &str,
) -> Result<(), AppError> {
    let path = root.join(INTERNAL_DIR).join(TRASH_DIR).join(TRASH_MANIFEST);
    let q = '"';
    let entry = format!(
        "{{{q}id{q}:{q}{}{q},{q}originalRelPath{q}:{q}{}{q},{q}isDir{q}:{},{q}deletedAtMs{q}:{},{q}trashRelPath{q}:{q}{}{q}}}",
        json_escape(id),
        json_escape(original_rel_path),
        is_dir,
        deleted_at_ms,
        json_escape(trash_rel_path),
    );
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let trimmed = existing.trim_end();
    let next = if trimmed.is_empty() {
        format!("[\n{entry}\n]\n")
    } else if let Some(head) = trimmed.strip_suffix(']') {
        let head = head.trim_end().trim_end_matches(',');
        format!("{head},\n{entry}\n]\n")
    } else {
        return Err(AppError::IoFailure(
            "回收站清单格式异常，已中止删除以避免记录丢失".to_string(),
        ));
    };
    let mut file = std::fs::File::create(&path)?;
    file.write_all(next.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch == '"' {
            out.push('\\');
            out.push('"');
        } else if ch == '\\' {
            out.push('\\');
            out.push('\\');
        } else if (ch as u32) < 0x20 {
            out.push_str(&format!("\\u{:04x}", ch as u32));
        } else {
            out.push(ch);
        }
    }
    out
}

/// 为已存在的目标找一个不冲突的同级名字：`name.md` → `name 1.md`（FR-ATTACH-05 的命名习惯）。
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn normalize_rel(rel: &str) -> String {
    rel.replace('\\', "/")
}

/// 把毫秒时间戳格式化为 `yyyy-MM`（UTC；不引入时间库，用标准民用历算法）。
pub(crate) fn yyyy_mm(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let (year, month) = civil_from_days(days);
    format!("{year:04}-{month:02}")
}

/// Howard Hinnant 的 days → (year, month) 算法（只取年月）。
fn civil_from_days(days: i64) -> (i64, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month)
}
