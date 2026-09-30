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

    if let Err(err) = replace(&tmp, target) {
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

fn replace(tmp: &Path, target: &Path) -> Result<(), AppError> {
    match fs::rename(tmp, target) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(AppError::FileExists(target.display().to_string()))
        }
        Err(err) => Err(AppError::from(err)),
    }
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
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn writes_and_reads_roundtrip() {
        let d = dir();
        let target = d.path().join("a.md");
        let mtime = write_note(&target, b"hello", None).unwrap();
        assert!(mtime > 0);
        assert_eq!(read_note(&target).unwrap(), b"hello");
    }

    #[test]
    fn overwrites_existing_content_atomically() {
        let d = dir();
        let target = d.path().join("a.md");
        atomic_write(&target, b"v1").unwrap();
        atomic_write(&target, b"v2-longer").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v2-longer");
    }

    #[test]
    fn leaves_no_temp_files_behind() {
        let d = dir();
        let target = d.path().join("a.md");
        for _ in 0..20 {
            atomic_write(&target, b"x").unwrap();
        }
        let names: Vec<String> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names.len(), 1, "不应残留临时文件：{names:?}");
    }

    #[test]
    fn conflict_detected_when_mtime_differs() {
        let d = dir();
        let target = d.path().join("a.md");
        atomic_write(&target, b"old").unwrap();
        let err = write_note(&target, b"new", Some(1)).unwrap_err();
        assert_eq!(err.code(), "E_WRITE_CONFLICT");
        assert_eq!(fs::read(&target).unwrap(), b"old", "冲突时必须保持原内容");
    }

    #[test]
    fn no_conflict_when_mtime_matches() {
        let d = dir();
        let target = d.path().join("a.md");
        let m = write_note(&target, b"old", None).unwrap();
        let m2 = write_note(&target, b"new", Some(m)).unwrap();
        assert!(m2 >= m);
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn read_missing_returns_file_not_found() {
        let d = dir();
        let err = read_note(&d.path().join("nope.md")).unwrap_err();
        assert_eq!(err.code(), "E_FILE_NOT_FOUND");
    }

    #[test]
    fn mtime_missing_file_errors() {
        let d = dir();
        assert!(mtime_ms(&d.path().join("nope.md")).is_err());
    }

    #[test]
    fn cleanup_removes_leftover_temp_files() {
        let d = dir();
        fs::write(d.path().join(format!("{TEMP_PREFIX}123-abc")), b"junk").unwrap();
        fs::write(d.path().join("keep.md"), b"data").unwrap();
        assert_eq!(cleanup_temp_files(d.path()).unwrap(), 1);
        assert!(d.path().join("keep.md").exists());
    }

    #[test]
    fn cleanup_on_missing_dir_is_zero() {
        assert_eq!(
            cleanup_temp_files(Path::new("definitely-not-here-xyz")).unwrap(),
            0
        );
    }

    #[test]
    fn creates_parent_directories() {
        let d = dir();
        let target = d.path().join("deep/nested/a.md");
        atomic_write(&target, b"x").unwrap();
        assert!(target.exists());
    }

    /// 可靠性：并发读取者绝不能看到半截内容（AC-REL-01 的本地等价物）。
    #[test]
    fn concurrent_readers_never_see_partial_content() {
        let d = dir();
        let target = d.path().join("note.md");
        let a = vec![b'A'; 32 * 1024];
        let bb = vec![b'B'; 32 * 1024];
        atomic_write(&target, &a).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let target = target.clone();
            let stop = stop.clone();
            let a = a.clone();
            let bb = bb.clone();
            std::thread::spawn(move || {
                let mut i = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    let data = if i.is_multiple_of(2) { &a } else { &bb };
                    atomic_write(&target, data).unwrap();
                    i += 1;
                }
            })
        };

        for _ in 0..300 {
            if let Ok(data) = fs::read(&target) {
                assert!(
                    data == a || data == bb,
                    "读到半截内容（长度 {}）",
                    data.len()
                );
            }
        }
        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();
        // 写入结束后应无临时文件残留
        let leftovers = fs::read_dir(d.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(TEMP_PREFIX)
            })
            .count();
        assert_eq!(leftovers, 0);
    }
}
