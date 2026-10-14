//! D-25 性能设施的**度量用例**（M5 批次「性能设施」，2026-10-13）。
//!
//! 约定：
//!   - 只**度量与打印**，不做严格阈值断言（严格判据由 `scripts/perf-d25.mjs` 按 PRD §6.1.2 的预算比对）；
//!   - 打印格式必须是 `D25 <判据 ID> key=value ...`（采样器按此解析）；
//!   - 夹具来源：环境变量 `KP_D25_VAULT` 指向 `scripts/gen-fixture-vault.mjs` 生成的库时直接使用，
//!     否则在临时目录里造一个小规模库 —— 这样 CI 跑得起，真机验收又能用真夹具。
//!
//! 覆盖现状见 `perf/d25-budgets.json` 的 `instrumented` 字段（未接入的判据由采样器显式打印为"待接入"）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::index_engine::{full_index_cancellable, index_file_mode, IndexMode, ScanEntry};
use crate::storage::index::open;
use crate::storage::pool::DbPool;

/// 造一篇笔记（含 frontmatter / 标题 / 链接 / 标签，贴近真实内容形状）。
fn write_note(root: &Path, rel: &str, i: usize) {
    let p = root.join(rel);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).expect("建目录");
    }
    let mut body = String::from("---\ntags: [a, b/c]\n---\n\n# 笔记 ");
    body.push_str(&i.to_string());
    body.push_str("\n\n");
    for k in 0..40 {
        body.push_str("内容 ");
        body.push_str(&(i * 131 + k).to_string());
        body.push_str(" 与 [[笔记 ");
        body.push_str(&((i + k) % 200).to_string());
        body.push_str("]] 以及 #标签/");
        body.push_str(&(i % 20).to_string());
        body.push('\n');
    }
    fs::write(p, body).expect("写笔记");
}

/// 夹具：优先用 `KP_D25_VAULT`（gen-fixture-vault.mjs 的产物），否则造一个临时小库。
fn d25_vault(notes: usize) -> (Option<tempfile::TempDir>, PathBuf) {
    if let Ok(p) = std::env::var("KP_D25_VAULT") {
        let path = PathBuf::from(p);
        assert!(path.is_dir(), "KP_D25_VAULT 不是目录：{path:?}");
        return (None, path);
    }
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..notes {
        write_note(&root, &format!("d{}/note-{i}.md", i % 20), i);
    }
    (Some(dir), root)
}

fn pool_for(root: &Path) -> DbPool {
    let db = root.join(".knowlpad").join("index.db");
    fs::create_dir_all(db.parent().expect("父目录")).expect("建 .knowlpad");
    open(&db).expect("打开索引库")
}

/// **NFR-PERF-03**：首次全量索引（含分词与 FTS 构建）。
#[test]
fn d25_nfr_perf_03_first_index() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    let started = Instant::now();
    let outcome =
        full_index_cancellable(&pool, &root, |_done, _total| {}, IndexMode::Force, || false)
            .expect("全量索引");
    let ms = started.elapsed().as_millis();
    let _ = &guard;
    println!(
        "D25 NFR-PERF-03-first-index ms={ms} notes={} indexed={} warnings={} engine_ms={}",
        notes,
        outcome.indexed,
        outcome.warnings.len(),
        outcome.duration_ms
    );
    // 宽松上限：只拦"数量级退化"（严格判据由采样器按 PRD 预算判定）。
    assert!(ms < 600_000, "首次全量索引 {ms} ms，已属数量级退化");
}

/// **AC-VAULT-05**：大 Vault 的进度事件与取消（取消必须被观测到，且发生在**批间**）。
#[test]
fn d25_ac_vault_05_progress_and_cancel() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    let events = AtomicUsize::new(0);
    let total_seen = AtomicUsize::new(0);
    let outcome = full_index_cancellable(
        &pool,
        &root,
        |_done, total| {
            events.fetch_add(1, Ordering::Relaxed);
            total_seen.store(total, Ordering::Relaxed);
        },
        IndexMode::Force,
        || events.load(Ordering::Relaxed) >= 1, // 第一批之后立刻要求取消
    )
    .expect("可取消的全量索引");
    let _ = &guard;
    println!(
        "D25 AC-VAULT-05-progress events={} total={} cancel-observed={} indexed={}",
        events.load(Ordering::Relaxed),
        total_seen.load(Ordering::Relaxed),
        outcome.cancelled,
        outcome.indexed
    );
    assert!(events.load(Ordering::Relaxed) >= 1, "至少要有一个进度事件");
    assert!(outcome.cancelled, "取消请求必须被观测到（AC-VAULT-05）");
}

/// **AC-REL-02 的助手入口**：被父用例以 `--exact perf_d25_tests::d25_ac_rel_02_kill_helper --ignored` 启动，
/// 索引到中途停在原地（父进程会在进度事件之间把它强杀）。
///
/// 为什么用 spawn-self：真正的"强杀"必须是**进程级**的（Windows 上没有安全的 fork）。
/// 该助手默认被 `#[ignore]` 跳过，只有父用例显式以 `--ignored` 启动它。
#[test]
#[ignore]
fn d25_ac_rel_02_kill_helper() {
    let Ok(vault) = std::env::var("KP_D25_KILL_VAULT") else {
        return; // 不是被父用例启动的，直接退出（避免误跑）
    };
    use std::io::Write;
    let root = PathBuf::from(vault);
    let pool = pool_for(&root);
    let outcome = full_index_cancellable(
        &pool,
        &root,
        |done, total| {
            // 打印进度并**稍作停顿**：给父进程一个可靠的"索引中"窗口来强杀。
            println!("D25-KILL-HELPER progress done={done} total={total}");
            let _ = std::io::stdout().flush();
            std::thread::sleep(std::time::Duration::from_millis(30));
        },
        IndexMode::Force,
        || false,
    )
    .expect("助手索引");
    println!("D25-KILL-HELPER finished indexed={}", outcome.indexed);
    let _ = std::io::stdout().flush();
    // 索引完成后仍停留一会儿：即使父进程没能在中途杀成，也不会让子进程变成孤儿。
    std::thread::sleep(std::time::Duration::from_secs(20));
}

/// 逐表计数（AC-REL-02 的"逐表一致"用）。
fn table_counts(pool: &DbPool) -> Vec<(String, i64)> {
    let tables = [
        "file",
        "heading",
        "block_id",
        "link",
        "tag",
        "file_tag",
        "file_alias",
        "note_fts",
    ];
    let mut out = Vec::new();
    for t in tables {
        let n: i64 = pool
            .with_reader(|c| {
                c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                    .map_err(|e| kp_domain::error::AppError::db_with("计数", &e))
            })
            .unwrap_or(-1);
        out.push((t.to_string(), n));
    }
    out
}

/// **AC-REL-02**：索引进行中（约 50%）强杀进程 → 重启后 SQLite 无损坏、重新索引后**逐表一致**。
#[test]
fn d25_ac_rel_02_kill_mid_index() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400usize);
    // ① 一次性完整索引的基线（同一份夹具的另一个副本）
    let (guard_base, base_root) = d25_vault(notes);
    let base_pool = pool_for(&base_root);
    full_index_cancellable(
        &base_pool,
        &base_root,
        |_, _| {},
        IndexMode::Force,
        || false,
    )
    .expect("基线索引");
    let baseline = table_counts(&base_pool);

    // ② 被杀的那一份
    let (guard_kill, kill_root) = d25_vault(notes);
    let killed_mid_index = kill_during_index(&kill_root);

    // ④ 重启：SQLite 必须无损坏
    let killed_pool = pool_for(&kill_root);
    let integrity: String = killed_pool
        .with_reader(|c| {
            c.query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(|e| kp_domain::error::AppError::db_with("integrity_check", &e))
        })
        .expect("重启后应能查询");

    // ⑤ 重新索引后与基线逐表一致
    full_index_cancellable(
        &killed_pool,
        &kill_root,
        |_, _| {},
        IndexMode::Force,
        || false,
    )
    .expect("重启后重索引");
    let after = table_counts(&killed_pool);
    let mismatched: Vec<String> = baseline
        .iter()
        .zip(after.iter())
        .filter(|(a, b)| a.1 != b.1)
        .map(|(a, b)| format!("{}:{}≠{}", a.0, a.1, b.1))
        .collect();
    let _ = (&guard_base, &guard_kill);
    println!(
        "D25 AC-REL-02-crash ok={} killed-mid={} integrity={} tables-equal={} mismatch={}",
        if killed_mid_index && integrity == "ok" && mismatched.is_empty() {
            1
        } else {
            0
        },
        killed_mid_index,
        integrity,
        mismatched.is_empty(),
        if mismatched.is_empty() {
            "-".to_string()
        } else {
            mismatched.join(",")
        }
    );
    assert_eq!(integrity, "ok", "强杀后 SQLite 必须无损坏");
    assert!(
        killed_mid_index,
        "未能在索引中途杀死助手进程（夹具太小？）—— 这不是通过"
    );
    assert!(
        mismatched.is_empty(),
        "重启重索引后与一次性索引不一致：{mismatched:?}"
    );
}

/// 从磁盘现读一个 `ScanEntry`（与生产扫描同源：rel_path + size + mtime）。
fn entry_of(root: &Path, rel: &str) -> ScanEntry {
    let abs = root.join(rel);
    let meta = fs::metadata(&abs).expect("stat");
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    ScanEntry {
        rel_path: rel.to_string(),
        size_bytes: meta.len(),
        mtime_ms,
    }
}

/// **NFR-PERF-10**：索引队列单文件增量处理（预算 50ms，三档同值）。
#[test]
fn d25_nfr_perf_10_single_file_incremental() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false).expect("首轮索引");

    // 改一篇（内容变长 ⇒ size 与 mtime 都变，避免命中 SkipUnchanged 的跳过分支）
    let rel = "d0/note-0.md";
    let mut body = fs::read_to_string(root.join(rel)).expect("读原笔记");
    body.push_str("\n新增内容 [[笔记 7]] #标签/3\n");
    fs::write(root.join(rel), &body).expect("写回");

    let entry = entry_of(&root, rel);
    // with_writer 的闭包必须 move（与 index_engine 里的 root_owned 同源）
    let root_owned = root.clone();
    let entry_owned = entry.clone();
    let started = Instant::now();
    let indexed = pool
        .with_writer(move |conn| index_file_mode(conn, &root_owned, &entry_owned, IndexMode::Force))
        .expect("单文件增量");
    let ms = started.elapsed().as_millis();
    let _ = &guard;
    println!("D25 NFR-PERF-10-incremental ms={ms} indexed={indexed} notes={notes}");
    assert!(indexed, "增量必须真的写入（返回 false 表示被跳过）");
}

/// **NFR-PERF-04 的 proxy**：搜索**功能在 M5 批次 1 才交付**，因此这里只测它将要用的 FTS5 原语
/// （`note_fts MATCH`）的延迟，并**明确标为 proxy**：它不含分词后的查询构造、排序与 UI 呈现。
/// 真正的 NFR-PERF-04 判据仍留在 `perf/d25-budgets.json` 的 pending 列表里。
#[test]
fn d25_nfr_perf_04_search_fts_proxy() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false).expect("索引");
    let term = format!("{}", 131 * (notes / 2) + 5); // 夹具里必然存在的数字词
    let started = Instant::now();
    let term_owned = term.clone();
    let hits = pool
        .with_reader(move |c| {
            c.query_row(
                "SELECT count(*) FROM note_fts WHERE note_fts MATCH ?1",
                rusqlite::params![term_owned],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| kp_domain::error::AppError::db_with("FTS 查询", &e))
        })
        .expect("FTS 查询应成功");
    let ms = started.elapsed().as_millis();
    let _ = &guard;
    println!("D25 NFR-PERF-04-search-fts-proxy ms={ms} hits={hits} term={term}");
}

/// 启动助手进程索引指定库，并在**索引中途**强杀它；返回是否真的杀在"进行中"。
///
/// 抽成独立函数有两个原因：① 让用例函数维持在 CODE-11 的函数级上限（100 SLOC）以内；
/// ② 让"子进程一定会被 wait 掉"只有一条路径（clippy 会检查这一点）。
fn kill_during_index(vault: &Path) -> bool {
    use std::io::BufRead;

    let exe = std::env::current_exe().expect("测试二进制路径");
    let mut child = std::process::Command::new(exe)
        .args([
            "--exact",
            "perf_d25_tests::d25_ac_rel_02_kill_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("KP_D25_KILL_VAULT", vault)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("启动助手进程");

    let mut killed_mid = false;
    if let Some(out) = child.stdout.take() {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(rest) = line.strip_prefix("D25-KILL-HELPER progress ") {
                let mut done = 0usize;
                let mut total = 0usize;
                for kv in rest.split_whitespace() {
                    if let Some(v) = kv.strip_prefix("done=") {
                        done = v.parse().unwrap_or(0);
                    }
                    if let Some(v) = kv.strip_prefix("total=") {
                        total = v.parse().unwrap_or(0);
                    }
                }
                if done > 0 && done < total {
                    killed_mid = true;
                    break;
                }
            }
            if line.contains("finished") {
                break;
            }
        }
    }
    // 唯一出口：无论走到哪条路径都杀 + wait（不留孤儿进程）。
    let _ = child.kill();
    let _ = child.wait();
    killed_mid
}
