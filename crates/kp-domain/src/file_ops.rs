//! 文件写操作（PRD §5.3.2；FR-FILE-10/11/13/15/30、FR-TRASH-02、NFR-REL-05、AC-FILE-03/04/AC-FILE-05 存储侧）。
//!
//! 纪律：
//! - **先备份后覆盖**：任何覆盖既有文件的操作，都必须先把原文件复制到 `.knowlpad/backup/<时间戳>/`
//!   （AC-FILE-04、PRD §6.2.1 步骤 4），并按 NFR-REL-05 保留最近 N 次（默认 10，**绝不淘汰最新一次**）；
//! - **不静默覆盖**：目标已存在时必须由调用方显式给出冲突策略（覆盖/重命名新建/取消）；
//! - **删除即软删除**：移入 `.knowlpad/trash/<yyyy-MM>/` 并在 `manifest.json` 记录原始相对路径
//!   （FR-TRASH-02/12——回收站不是纯派生数据，`.knowlpad/` 被删则一并销毁）；
//! - 全部写入经 `note_io`/`fs_atomic` 的原子协议（R-06）；路径全部经 `PathGuard`（含逐段名校验，FR-FILE-12）。

use crate::error::AppError;
use crate::file_trash::{
    backup_existing, move_to_trash, normalize_rel, now_ms, prune_backups, BACKUP_KEEP_DEFAULT,
};
use crate::fs_atomic;
use crate::note_io;
use crate::path_guard::PathGuard;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Overwrite,
    RenameNew,
    Cancel,
}
impl ConflictPolicy {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "overwrite" => Ok(Self::Overwrite),
            "renameNew" => Ok(Self::RenameNew),
            "cancel" => Ok(Self::Cancel),
            other => Err(AppError::IoFailure(format!("未知的冲突处理方式：{other}"))),
        }
    }
}

/// 新建/写入结果（PRD §5.3.2 的 WriteResult）。
#[derive(Debug, Clone, PartialEq, Eq)]

pub struct WriteOutcome {
    pub rel_path: String,
    pub new_mtime_ms: i64,
    /// 覆盖既有文件时，原文件的备份相对路径（供上层写审计日志）
    pub backed_up_to: Option<String>,
}

/// 重命名/移动结果（PRD §5.3.2.1 的 RenameResult）。
#[derive(Debug, Clone, PartialEq, Eq)]

pub struct RenameOutcome {
    pub from: String,
    pub to: String,
    pub new_mtime_ms: i64,
    /// 覆盖既有文件时，原文件的备份相对路径（供上层写审计日志）
    pub backed_up_to: Option<String>,
}

/// 删除结果（PRD §5.3.2.1 的 DeleteResult）。
#[derive(Debug, Clone, PartialEq, Eq)]

pub struct DeleteOutcome {
    pub rel_path: String,
    pub trashed_count: u64,
    pub trash_rel_path: String,
}

/// 新建笔记（FR-FILE-10/13）。目标已存在时按 `policy` 处理。
pub fn create_note(
    root: &Path,
    rel_path: &str,
    content: &str,
    policy: ConflictPolicy,
) -> Result<WriteOutcome, AppError> {
    let guard = PathGuard::new(root)?;
    let abs = guard.resolve(rel_path)?;
    let mut backed_up_to = None;
    if abs.exists() {
        match policy {
            // 取消：不改动任何东西（AC-FILE-04 的「取消」路径）
            ConflictPolicy::Cancel => return Err(AppError::FileExists(rel_path.to_string())),
            ConflictPolicy::Overwrite => {
                backed_up_to = Some(backup_existing(&guard, &abs, rel_path, now_ms())?);
                prune_backups(guard.canonical_root(), BACKUP_KEEP_DEFAULT)?;
            }
            ConflictPolicy::RenameNew => {
                let (unique_abs, unique_rel) = unique_sibling(&guard, &abs, rel_path)?;
                let mtime = note_io::write_note(&unique_abs, content.as_bytes(), None)?;
                return Ok(WriteOutcome {
                    rel_path: unique_rel,
                    new_mtime_ms: mtime,
                    backed_up_to: None,
                });
            }
        }
    } else if let Some(parent) = abs.parent() {
        if !parent.is_dir() {
            return Err(AppError::FileNotFound(format!("目录不存在：{rel_path}")));
        }
    }
    let mtime = note_io::write_note(&abs, content.as_bytes(), None)?;
    Ok(WriteOutcome {
        rel_path: normalize_rel(rel_path),
        new_mtime_ms: mtime,
        backed_up_to,
    })
}

/// 新建文件夹（FR-FILE-11：支持 `a/b/c` 多级一次创建）。已存在同名目录时幂等返回。
pub fn create_folder(root: &Path, rel_path: &str) -> Result<(), AppError> {
    let guard = PathGuard::new(root)?;
    let abs = guard.resolve(rel_path)?;
    if abs.exists() {
        if abs.is_dir() {
            return Ok(());
        }
        return Err(AppError::FileExists(format!(
            "{rel_path} 已存在且不是文件夹"
        )));
    }
    std::fs::create_dir_all(&abs)?;
    Ok(())
}

/// 重命名 / 移动（**不含链接改写**，改写属 M4）。
pub fn rename_path(
    root: &Path,
    from: &str,
    to: &str,
    policy: ConflictPolicy,
) -> Result<RenameOutcome, AppError> {
    let guard = PathGuard::new(root)?;
    let src = guard.resolve(from)?;
    if !src.exists() {
        return Err(AppError::FileNotFound(from.to_string()));
    }
    let mut dst = guard.resolve(to)?;
    let mut out_rel = normalize_rel(to);
    let mut backed_up_to = None;
    // 目录不得移入自身子树（否则会自我吞噬）
    if src.is_dir() && dst.starts_with(&src) {
        return Err(AppError::IoFailure(
            "不能把文件夹移动到它自己的子目录".to_string(),
        ));
    }
    if let Some(parent) = dst.parent() {
        if !parent.is_dir() {
            return Err(AppError::FileNotFound(format!("目标目录不存在：{to}")));
        }
    }
    if dst.exists() {
        match policy {
            ConflictPolicy::Cancel => return Err(AppError::FileExists(to.to_string())),
            ConflictPolicy::RenameNew => {
                let (unique_abs, unique_rel) = unique_sibling(&guard, &dst, to)?;
                dst = unique_abs;
                out_rel = unique_rel;
            }
            ConflictPolicy::Overwrite => {
                if dst.is_dir() {
                    // 只允许覆盖**空目录**：非空目录覆盖等于静默销毁数据（R-07）
                    if std::fs::read_dir(&dst)?.next().is_some() {
                        return Err(AppError::FileExists(format!(
                            "{to} 是非空文件夹，不允许覆盖（可先改名或清空）"
                        )));
                    }
                    std::fs::remove_dir(&dst)?;
                } else {
                    backed_up_to = Some(backup_existing(&guard, &dst, to, now_ms())?);
                    prune_backups(guard.canonical_root(), BACKUP_KEEP_DEFAULT)?;
                }
            }
        }
    }
    if src.is_dir() {
        std::fs::rename(&src, &dst)?;
    } else {
        fs_atomic::replace(&src, &dst)?;
    }
    let mtime = note_io::mtime_ms(&dst).unwrap_or(0);
    Ok(RenameOutcome {
        from: normalize_rel(from),
        to: out_rel,
        new_mtime_ms: mtime,
        backed_up_to,
    })
}

/// 删除 = 软删除（FR-FILE-30）：移入 `.knowlpad/trash/<yyyy-MM>/` 并登记 manifest。
pub fn delete_path(
    root: &Path,
    rel_path: &str,
    recursive: bool,
) -> Result<DeleteOutcome, AppError> {
    let guard = PathGuard::new(root)?;
    let abs = guard.resolve(rel_path)?;
    if !abs.exists() {
        return Err(AppError::FileNotFound(rel_path.to_string()));
    }
    let is_dir = abs.is_dir();
    if is_dir && !recursive && std::fs::read_dir(&abs)?.next().is_some() {
        return Err(AppError::FileExists(format!(
            "{rel_path} 不是空文件夹，需要确认递归删除"
        )));
    }
    let trashed_count = if is_dir { count_entries(&abs)? } else { 1 };
    // 备份/回收站的存储细节见 file_trash（布局与清单格式在那里集中维护）
    let trash_rel_path = move_to_trash(guard.canonical_root(), &abs, rel_path, is_dir)?;
    Ok(DeleteOutcome {
        rel_path: normalize_rel(rel_path),
        trashed_count,
        trash_rel_path,
    })
}

/// 覆盖前备份原文件到 `.knowlpad/backup/<时间戳>/<相对路径>`（AC-FILE-04、PRD §6.2.1 步骤 4）。
fn unique_sibling(guard: &PathGuard, abs: &Path, rel: &str) -> Result<(PathBuf, String), AppError> {
    let (stem, ext) = split_name(abs, rel);
    let parent_rel = rel
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default();
    for index in 1..=9999u32 {
        let candidate_name = if ext.is_empty() {
            format!("{stem} {index}")
        } else {
            format!("{stem} {index}.{ext}")
        };
        let candidate_rel = if parent_rel.is_empty() {
            candidate_name
        } else {
            format!("{parent_rel}/{candidate_name}")
        };
        let candidate_abs = guard.resolve(&candidate_rel)?;
        if !candidate_abs.exists() {
            return Ok((candidate_abs, candidate_rel));
        }
    }
    Err(AppError::FileExists(format!("{rel} 的可选新名已用尽")))
}

fn split_name(abs: &Path, rel: &str) -> (String, String) {
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            rel.rsplit_once('/')
                .map(|(_, n)| n.to_string())
                .unwrap_or_else(|| rel.to_string())
        });
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), ext.to_string()),
        _ => (name, String::new()),
    }
}

/// 递归统计条目数（文件与目录都计入）。
fn count_entries(dir: &Path) -> Result<u64, AppError> {
    let mut count = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            count += 1;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(entry.path());
            }
        }
    }
    Ok(count)
}

#[cfg(test)]
#[path = "file_ops_tests.rs"]
mod tests;
