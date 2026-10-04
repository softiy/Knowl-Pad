//! 文件树只读层（FR-FILE-01/02/03/05/12、AC-FILE-09；PRD §5.3.2.1 的 FileNode / FileStat / ValidationResult）。
//!
//! 纪律：
//! - **单层**返回（禁止一次返回全量树，IPC-05 的 4MB 约束）；
//! - 隐藏规则与 AC-FILE-09 逐条对应：`.knowlpad/` **永远隐藏**；其它以 `.` 开头的条目默认隐藏、
//!   开启「显示隐藏文件」后可见（**含 `.obsidian/`、`.git/`**——它们只是默认隐藏，不是禁止显示）；
//! - 遍历**不跟随符号链接**（避免越出 Vault 或成环）；
//! - 路径一律经 `PathGuard` 七步校验（SEC-02）。

use crate::error::AppError;
use crate::path_guard::{validate_segment, PathGuard};
use crate::vault_paths::INTERNAL_DIR;
use std::path::{Path, PathBuf};

/// 文件类型（PRD §5.3.2.1 的 `kind`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Note,
    Attachment,
    Other,
}

impl FileKind {
    /// IPC 传输值（camelCase 契约见技术方案 §8.4）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Attachment => "attachment",
            Self::Other => "other",
        }
    }
}

/// 图片类附件扩展名（FR-ATTACH-01）。
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"];
/// 音视频类附件扩展名（FR-ATTACH-01）。
pub const MEDIA_EXTS: &[&str] = &[
    "mp4", "mp3", "wav", "m4a", "ogg", "flac", "mov", "mkv", "webm", "avi",
];
/// 文档类附件扩展名。
pub const DOC_EXTS: &[&str] = &["pdf"];

/// 按扩展名分类（小写比较）。
pub fn classify(ext: &str) -> FileKind {
    let ext = ext.to_ascii_lowercase();
    if ext == "md" {
        return FileKind::Note;
    }
    if IMAGE_EXTS.contains(&ext.as_str())
        || MEDIA_EXTS.contains(&ext.as_str())
        || DOC_EXTS.contains(&ext.as_str())
    {
        return FileKind::Attachment;
    }
    FileKind::Other
}

/// 单层目录条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub rel_path: String,
    pub name: String,
    pub is_dir: bool,
    pub kind: FileKind,
    pub size_bytes: Option<u64>,
    pub mtime_ms: Option<i64>,
    /// 仅目录有值：是否含**可见**子项（决定展开箭头）
    pub has_children: Option<bool>,
}

/// `.knowlpad/` 永远隐藏（内部数据目录；AC-FILE-09）。
fn is_always_hidden(name: &str) -> bool {
    name == INTERNAL_DIR
}

/// 默认隐藏的条目：以 `.` 开头（含 `.obsidian/`、`.git/`）。
fn is_dot_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// 单层列出目录（**不递归**）。`rel_dir` 为空串表示 Vault 根。
pub fn list_dir(
    root: &Path,
    rel_dir: &str,
    include_hidden: bool,
) -> Result<Vec<FileEntry>, AppError> {
    let guard = PathGuard::new(root)?;
    let dir = if rel_dir.is_empty() {
        guard.canonical_root().to_path_buf()
    } else {
        guard.resolve(rel_dir)?
    };
    if !dir.is_dir() {
        return Err(AppError::FileNotFound(format!("{rel_dir} 不是目录")));
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_always_hidden(&name) {
            continue;
        }
        if !include_hidden && is_dot_hidden(&name) {
            continue;
        }
        // file_type 不解析符号链接：软链/联接既不展开也不计入
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let metadata = entry.metadata().ok();
        let is_dir = file_type.is_dir();
        let rel_path = relative_of(&guard, &path, &name);
        let kind = if is_dir {
            FileKind::Other
        } else {
            classify(
                path.extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .as_str(),
            )
        };
        entries.push(FileEntry {
            rel_path,
            name,
            is_dir,
            kind,
            size_bytes: metadata.as_ref().filter(|_| !is_dir).map(|m| m.len()),
            mtime_ms: metadata
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(mtime_to_ms),
            has_children: if is_dir {
                Some(has_visible_child(&path, include_hidden))
            } else {
                None
            },
        });
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// 单文件/目录元信息。
pub fn stat(root: &Path, rel_path: &str) -> Result<FileEntry, AppError> {
    let guard = PathGuard::new(root)?;
    let path = guard.resolve(rel_path)?;
    if !path.exists() {
        return Err(AppError::FileNotFound(rel_path.to_string()));
    }
    let metadata = std::fs::metadata(&path)?;
    let is_dir = metadata.is_dir();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| rel_path.to_string());
    Ok(FileEntry {
        rel_path: rel_path.replace('\\', "/"),
        name,
        is_dir,
        kind: if is_dir {
            FileKind::Other
        } else {
            classify(
                path.extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .as_str(),
            )
        },
        size_bytes: if is_dir { None } else { Some(metadata.len()) },
        mtime_ms: metadata.modified().ok().and_then(mtime_to_ms),
        has_children: None,
    })
}

/// 文件名合法性校验结果（FR-FILE-12）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameValidation {
    pub valid: bool,
    /// 非法原因：**中文、具体、可操作**（ERR-02）
    pub reason: Option<String>,
}

/// 校验单个文件名（不是路径）：拒绝非法字符、控制字符、首尾空格/点号、Windows 保留名、编码穿越。
pub fn validate_name(name: &str) -> NameValidation {
    match validate_segment(name) {
        Ok(()) => NameValidation {
            valid: true,
            reason: None,
        },
        Err(err) => NameValidation {
            valid: false,
            reason: Some(reason_of(&err)),
        },
    }
}

/// 从领域错误中提取**面向用户**的中文原因。
fn reason_of(err: &AppError) -> String {
    match err {
        AppError::InvalidFilename(detail) => detail.clone(),
        AppError::PathEscapeDeny => "文件名含编码后的路径穿越序列，已被拒绝".to_string(),
        other => other.to_string(),
    }
}

/// 目录是否含**可见**子项（读到第一个即返回，避免整层扫描）。
fn has_visible_child(dir: &Path, include_hidden: bool) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_always_hidden(&name) {
            continue;
        }
        if !include_hidden && is_dot_hidden(&name) {
            continue;
        }
        if matches!(entry.file_type(), Ok(t) if !t.is_symlink()) {
            return true;
        }
    }
    false
}

/// 目录优先；其次按名称**不区分大小写**排序（同名不同大小写时按原串稳定收敛）。
fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn relative_of(guard: &PathGuard, path: &Path, name: &str) -> String {
    path.strip_prefix(guard.canonical_root())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| name.to_string())
}

fn mtime_to_ms(modified: std::time::SystemTime) -> Option<i64> {
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// 供命令层复用的绝对路径解析（避免各命令自行拼接）。
pub fn resolve_dir(root: &Path, rel_dir: &str) -> Result<PathBuf, AppError> {
    let guard = PathGuard::new(root)?;
    if rel_dir.is_empty() {
        Ok(guard.canonical_root().to_path_buf())
    } else {
        guard.resolve(rel_dir)
    }
}

#[cfg(test)]
#[path = "file_tree_tests.rs"]
mod tests;
