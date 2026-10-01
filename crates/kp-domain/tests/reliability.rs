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

// ─────────────────────────────────────────────────────────────────────────────
// AC-REL-01：断电/强杀一致性（M1 DoD 要求）
//
// 做法：把本测试二进制当作「写入子进程」再次拉起（--exact + --ignored），
// 让其持续原子写入，父进程在随机时刻**强杀**它（unix: SIGKILL / Windows: TerminateProcess），
// 然后按 AC-REL-01 逐项校验不变量：
//   ① 目标文件必须是「完整旧内容」或「完整新内容」；
//   ② 不得出现零字节文件；
//   ③ 模拟重启时的清理后，用户目录不得残留 .kp-tmp-*。
//
// 默认轮数 24（CI 友好）；AC-REL-01 的全量 1000 次见下方 #[ignore] 用例：
//   cargo test -p kp-domain --test reliability -- --ignored ac_rel_01_full_scale
// ─────────────────────────────────────────────────────────────────────────────

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CHILD_ENV: &str = "KP_RELIABILITY_CHILD";
const PAYLOAD_LEN: usize = 64 * 1024;

fn payloads() -> (Vec<u8>, Vec<u8>) {
    (vec![b'A'; PAYLOAD_LEN], vec![b'B'; PAYLOAD_LEN])
}

/// 子进程模式：持续原子写入，直到被父进程强杀。
#[test]
#[ignore = "仅供崩溃测试通过 KP_RELIABILITY_CHILD 自举调用"]
fn crash_child_writer() {
    let Ok(target) = std::env::var(CHILD_ENV) else {
        return; // 直接运行（无环境变量）时不做任何事
    };
    let target = PathBuf::from(target);
    let (a, b) = payloads();
    let mut round = 0usize;
    loop {
        let data = if round.is_multiple_of(2) { &a } else { &b };
        if atomic_write(&target, data).is_err() {
            // 极端情况下（例如被占用）子进程退出即可，父进程仍会校验不变量
            return;
        }
        round += 1;
    }
}

/// 简易 LCG：避免为随机等待引入新依赖。
fn next_rand(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

fn seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos | 1
}

fn count_temp_files(dir: &std::path::Path) -> usize {
    fs::read_dir(dir)
        .expect("目录应可读")
        .filter(|entry| {
            entry
                .as_ref()
                .map(|e| e.file_name().to_string_lossy().starts_with(TEMP_PREFIX))
                .unwrap_or(false)
        })
        .count()
}

/// 强杀写入进程 N 轮，每轮都校验 AC-REL-01 的三条不变量。
fn run_kill_rounds(iterations: usize) {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let target = dir.path().join("note.md");
    let (a, b) = payloads();
    atomic_write(&target, &a).expect("初始写入应成功");

    let exe = std::env::current_exe().expect("应可定位测试二进制");
    let mut state = seed();
    // 防「空转」：必须观察到子进程确实写入过新内容，否则本测试没有证明力
    let mut saw_new_content = 0usize;

    for round in 0..iterations {
        let mut child = Command::new(&exe)
            .args(["--exact", "crash_child_writer", "--ignored", "--nocapture"])
            .env(CHILD_ENV, target.to_string_lossy().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("应可启动写入子进程");

        // 随机等待 5..80ms：覆盖「写临时文件」「fsync」「rename」各个阶段
        let wait_ms = 5 + next_rand(&mut state) % 76;
        std::thread::sleep(Duration::from_millis(wait_ms));
        let _ = child.kill(); // unix: SIGKILL；Windows: TerminateProcess
        let _ = child.wait();

        // ① 完整旧内容或完整新内容（绝无半截）
        let data = fs::read(&target).expect("目标文件必须始终存在");
        assert!(
            data == a || data == b,
            "第 {round} 轮（等待 {wait_ms}ms）读到半截内容：{} 字节",
            data.len()
        );
        if data == b {
            saw_new_content += 1;
        }
        // ② 不得零字节
        assert!(!data.is_empty(), "第 {round} 轮出现零字节文件");

        // ③ 模拟重启清理：清理后用户目录不得残留临时文件
        cleanup_temp_files(dir.path()).expect("清理应成功");
        assert_eq!(
            count_temp_files(dir.path()),
            0,
            "第 {round} 轮清理后仍有 .kp-tmp-* 残留"
        );
    }

    assert!(
        saw_new_content > 0,
        "整个测试期间从未观察到新内容——子进程可能没有真正写入，测试无证明力"
    );
}

#[test]
fn ac_rel_01_kill_during_write_never_corrupts() {
    let iterations = std::env::var("KP_CRASH_ITER")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(24);
    run_kill_rounds(iterations);
}

#[test]
#[ignore = "AC-REL-01 全量 1000 次强杀（耗时数分钟，按需手动运行）"]
fn ac_rel_01_full_scale() {
    run_kill_rounds(1000);
}
