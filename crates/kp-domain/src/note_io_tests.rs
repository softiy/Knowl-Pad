//! note_io 的单元测试（CODE-11：测试位于独立文件）。

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

#[test]
fn recursive_cleanup_removes_nested_leftovers_only() {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let nested = dir.path().join("a/b/c");
    fs::create_dir_all(&nested).expect("应可建目录");
    fs::create_dir_all(dir.path().join(".knowlpad")).expect("应可建内部目录");
    fs::write(dir.path().join(format!("{TEMP_PREFIX}root")), b"junk").expect("应可写");
    fs::write(nested.join(format!("{TEMP_PREFIX}deep")), b"junk").expect("应可写");
    fs::write(dir.path().join(".knowlpad/index.db-wal"), b"keep").expect("应可写");
    fs::write(dir.path().join("real.md"), b"keep").expect("应可写");

    let removed = cleanup_temp_files_recursive(dir.path()).expect("清理应成功");
    assert_eq!(removed, 2, "两处残留都应被删除");
    assert!(dir.path().join("real.md").exists(), "正常文件必须保留");
    assert!(
        dir.path().join(".knowlpad/index.db-wal").exists(),
        ".knowlpad 内不得被触碰"
    );
}

#[cfg(unix)]
#[test]
fn recursive_cleanup_does_not_follow_symlinks() {
    let outside = tempfile::tempdir().expect("外部目录应可创建");
    let victim = outside.path().join(format!("{TEMP_PREFIX}victim"));
    fs::write(&victim, b"must survive").expect("应可写");
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).expect("应可建软链");

    let removed = cleanup_temp_files_recursive(dir.path()).expect("清理应成功");
    assert_eq!(removed, 0, "不得沿符号链接删除外部文件");
    assert!(victim.exists(), "Vault 外文件必须完好");
}

#[test]
fn recursive_cleanup_on_missing_dir_is_zero() {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    assert_eq!(
        cleanup_temp_files_recursive(&dir.path().join("nope")).expect("应成功"),
        0
    );
}
