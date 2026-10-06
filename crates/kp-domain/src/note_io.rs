use crate::error::AppError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 原子写入的临时文件前缀（用户目录中短暂出现，启动时清理）。
pub const TEMP_PREFIX: &str = ".kp-tmp-";

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 文件修改时间（毫秒）。用于增量索引与保存冲突检测（base_mtime）。
pub fn mtime_ms(path: &Path) -> Result<i64, AppError> {
    let meta = fs::metadata(path)?;
    let modified = meta.modified()?;
    let elapsed = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(elapsed.as_millis() as i64)
}

fn temp_path(parent: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{TEMP_PREFIX}{}-{nanos}-{seq}", std::process::id()))
}

/// 原子写入协议（§6.3）：同目录临时文件 → write+fsync → rename → 父目录 fsync。
/// 任何时刻磁盘上要么是完整旧内容、要么是完整新内容（NFR-REL-01）。
pub fn atomic_write(target: &Path, content: &[u8]) -> Result<(), AppError> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::IoFailure("无父目录".into()))?;
    fs::create_dir_all(parent)?;

    let tmp = temp_path(parent);
    if let Err(err) = write_and_sync(&tmp, content) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    // 记录既有权限位，rename 后恢复，避免静默改变用户文件权限
    #[cfg(unix)]
    let prev_perms = fs::metadata(target).ok().map(|m| m.permissions());

    if let Err(err) = crate::fs_atomic::replace(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    #[cfg(unix)]
    if let Some(perms) = prev_perms {
        let _ = fs::set_permissions(target, perms);
    }

    Ok(())
}

fn write_and_sync(tmp: &Path, content: &[u8]) -> Result<(), AppError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)?;
    file.write_all(content)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

/// 递归清理 Vault 内的 .kp-tmp-* 残留（技术方案 §6.3「启动时扫描 Vault 清理残留」）。
///
/// 安全约束：
/// - **不跟随符号链接**（Windows 的 junction/软链同样被跳过），避免删除 Vault 外的文件；
/// - 跳过 .knowlpad/（内部目录，其残留不影响用户可见目录）；
/// - 单个条目失败不中断整体，返回成功删除的数量。
pub fn cleanup_temp_files_recursive(root: &Path) -> Result<usize, AppError> {
    if !root.is_dir() {
        return Ok(0);
    }
    let mut removed = 0usize;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            // 目录不可读（权限等）不应让整个清理失败
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // 关键：不跟随符号链接（file_type 不解析符号链接）
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                // 内部目录无需清理；第三方配置目录/VCS 目录**整目录跳过**（FR-STORAGE-02：
                // 不得修改、移动或删除其中任何内容——连清理我们自己的临时文件也算"删除其中内容"）
                if name == crate::vault_paths::INTERNAL_DIR
                    || crate::vault_paths::NEVER_TOUCH_DIRS.contains(&name.as_str())
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(TEMP_PREFIX) && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// 读取笔记；不存在时返回 E_FILE_NOT_FOUND。
pub fn read_note(target: &Path) -> Result<Vec<u8>, AppError> {
    if !target.exists() {
        return Err(AppError::FileNotFound(target.display().to_string()));
    }
    Ok(fs::read(target)?)
}

/// 写入笔记：先做 base_mtime 冲突检测（FR-EDITOR-34），再原子写入，返回新 mtime。
pub fn write_note(target: &Path, content: &[u8], base_mtime: Option<i64>) -> Result<i64, AppError> {
    // 保存**不得**创建父目录：文件或所在目录被删除后，自动保存不能让它"复活"
    // （否则回收站清单与磁盘状态不一致；新建走 create_note，那条路径允许创建）。
    if let Some(parent) = target.parent() {
        if !parent.is_dir() {
            return Err(AppError::FileNotFound(target.display().to_string()));
        }
    }
    if let Some(base) = base_mtime {
        if target.exists() {
            let actual = mtime_ms(target)?;
            if actual != base {
                return Err(AppError::WriteConflict(target.display().to_string()));
            }
        }
    }
    atomic_write(target, content)?;
    mtime_ms(target)
}

/// 清理目录中残留的临时文件，返回清理数量（启动时调用）。
pub fn cleanup_temp_files(dir: &Path) -> Result<usize, AppError> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(TEMP_PREFIX) {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "note_io_tests.rs"]
mod tests;
