//! 可靠性测试集（AC-REL-01 的本地等价物）：原子写入在并发读取下绝不产生半截内容。
use kp_domain::note_io::{atomic_write, cleanup_temp_files, TEMP_PREFIX};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[test]
fn concurrent_readers_never_see_partial_content() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("note.md");
    let a = vec![b'A'; 64 * 1024];
    let b = vec![b'B'; 64 * 1024];
    atomic_write(&target, &a).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let target = target.clone();
        let stop = stop.clone();
        let a = a.clone();
        let b = b.clone();
        std::thread::spawn(move || {
            let mut i = 0usize;
            while !stop.load(Ordering::Relaxed) {
                let data = if i.is_multiple_of(2) { &a } else { &b };
                atomic_write(&target, data).unwrap();
                i += 1;
            }
        })
    };

    for _ in 0..500 {
        let data = fs::read(&target).unwrap();
        assert!(
            data == a || data == b,
            "读到半截内容（{} 字节）",
            data.len()
        );
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();

    let leftover = fs::read_dir(dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(TEMP_PREFIX)
        })
        .count();
    assert_eq!(leftover, 0, "写入结束后不应残留临时文件");
}

#[test]
fn cleanup_removes_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(format!("{TEMP_PREFIX}deadbeef")), b"junk").unwrap();
    fs::write(dir.path().join("keep.md"), b"data").unwrap();
    assert_eq!(cleanup_temp_files(dir.path()).unwrap(), 1);
    assert!(dir.path().join("keep.md").exists());
}
