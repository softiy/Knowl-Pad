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
use crate::storage::index_query::list_children;
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
    // 口径（PRD §6.1.4）：**判定用批内处理时间**（不含索引库持久化）；含提交的墙钟另测并记录。
    let ms_with_commit = {
        let root_in = root_owned.clone();
        let entry_in = entry_owned.clone();
        let t0 = Instant::now();
        let ok = pool
            .with_writer(move |conn| index_file_mode(conn, &root_in, &entry_in, IndexMode::Force))
            .expect("单文件增量");
        let wall = t0.elapsed().as_millis();
        assert!(ok, "增量必须真的写入（返回 false 表示被跳过）");
        wall
    };
    let ms_in_batch = {
        let root_in = root_owned.clone();
        let entry_in = entry_owned.clone();
        let t0 = Instant::now();
        pool.with_writer(move |conn| {
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| kp_domain::error::AppError::db_with("开启事务", &e))?;
            let ok = index_file_mode(conn, &root_in, &entry_in, IndexMode::Force)?;
            conn.execute_batch("COMMIT")
                .map_err(|e| kp_domain::error::AppError::db_with("提交事务", &e))?;
            Ok(ok)
        })
        .expect("批内单文件增量");
        t0.elapsed().as_millis()
    };
    let ms = ms_in_batch;
    let _ = &guard;
    println!("D25 NFR-PERF-10-incremental ms={ms} ms_with_commit={ms_with_commit} notes={notes}");
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

/// **NFR-PERF-02**：打开 Vault 到文件树可用（**索引已存在**）—— 后端部分 = 打开索引库 + 文件树首屏。
///
/// 不含的部分（登记在 `perf/d25-budgets.json`）：Tauri 窗口创建、前端首屏渲染、IPC 往返。
#[test]
fn d25_nfr_perf_02_cold_open() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    {
        let pool = pool_for(&root);
        full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false)
            .expect("先建好索引");
    } // 放下连接：下面测的是"重新打开"

    let db = root.join(".knowlpad").join("index.db");
    let t0 = Instant::now();
    let pool = open(&db).expect("打开索引库");
    let open_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let root_entries = list_children(&pool, "").expect("列举根");
    let list_ms = t1.elapsed().as_millis();
    let _ = &guard;
    println!(
        "D25 NFR-PERF-02-cold-open ms={} open_ms={open_ms} list_ms={list_ms} entries={} notes={notes}",
        open_ms + list_ms,
        root_entries.len()
    );
    assert!(!root_entries.is_empty(), "索引已存在时根目录不该为空");
}

/// **NFR-PERF-11 的 proxy**：外部新增一个文件 → 增量扫描（`SkipUnchanged`）→ 索引可见。
///
/// **proxy 的理由**：它不含"文件监听触发延迟"与"到 UI 反映"，而 PRD 的判据是端到端的。
/// 真正的 NFR-PERF-11 仍留在 pending；这条只回答"外部变更后，索引侧要多久才可见"。
#[test]
fn d25_nfr_perf_11_external_change_proxy() {
    let notes = std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize);
    let (guard, root) = d25_vault(notes);
    let pool = pool_for(&root);
    full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false).expect("首轮索引");
    let before = list_children(&pool, "d0").expect("列举 d0").len();

    // 外部变更：直接用 fs 写（模拟别的程序改盘，不经过本进程的写命令）
    write_note(&root, "d0/note-external.md", 9999);
    let started = Instant::now();
    let outcome =
        full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::SkipUnchanged, || false)
            .expect("增量扫描");
    let visible_ms = started.elapsed().as_millis();
    let after = list_children(&pool, "d0").expect("再列举 d0").len();
    let _ = (&guard, &outcome);
    println!(
        "D25 NFR-PERF-11-external-change-proxy ms={visible_ms} visible={} d0_before={before} d0_after={after} scanned={} skipped={}",
        after > before,
        outcome.indexed,
        outcome.skipped
    );
    assert!(
        after > before,
        "外部新增的文件必须在索引里可见（proxy 的核心断言）"
    );
}

// ── M5 批次 5：搜索域的真测（NFR-PERF-04 后端部分 / line: 候选上限标定）──────────

use std::sync::Arc;

use crate::commands::search::{
    PageArg, SearchFulltextArgs, SearchOptions, MAX_LINE_FILTER_CANDIDATES,
};
use crate::commands::search_index::search_fulltext_impl;
use crate::commands::search_page::{page_with_line_filter, PageWindow, ViewOptions};
use crate::state::AppState;
use crate::storage::search_query::{self, SortOrder};
use kp_domain::search::{parse_query, MatchMode};

/// 夹具规模（与既有 d25 用例同一约定：KP_D25_NOTES，缺省 300）。
fn d25_notes_arg() -> usize {
    std::env::var("KP_D25_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300usize)
}

/// 索引夹具并装配 AppState（搜索命令的入口需要 state）。
///
/// **索引库放在自己的临时目录里**（不是 `<vault>/.knowlpad`）：同一个夹具会被多个 d25 用例共用，
/// 若都往夹具目录里的那个库写，并行跑就会互相踩（实测：两个用例同时全量索引 ⇒ `全量索引` 直接失败）。
/// 夹具文件只被**只读**访问，因此各用各的库既能并行又互不影响。
///
/// 返回的 `TempDir` 是索引库的**生命周期守卫**，调用方必须持有到用例结束。
fn d25_index_and_state(root: &Path) -> (tempfile::TempDir, Arc<DbPool>, AppState) {
    let db_dir = tempfile::tempdir().expect("索引库临时目录");
    let pool = Arc::new(open(&db_dir.path().join("index.db")).expect("打开索引库"));
    full_index_cancellable(&pool, root, |_, _| {}, IndexMode::Force, || false).expect("全量索引");
    let state = AppState::new();
    state.set_root(Some(root.to_path_buf()));
    state.set_index_db(Some(pool.clone()));
    (db_dir, pool, state)
}

/// 索引里的笔记数（**从库里取**，不靠环境变量自报）。
fn d25_note_count(pool: &DbPool) -> i64 {
    pool.with_reader(|c| {
        c.query_row(
            "SELECT count(*) FROM file WHERE deleted = 0 AND kind = 'note'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| kp_domain::error::AppError::db_with("统计笔记数", &e))
    })
    .expect("统计笔记数")
}

fn d25_search_args(query: &str) -> SearchFulltextArgs {
    SearchFulltextArgs {
        query: query.to_string(),
        options: SearchOptions::default(),
        page: PageArg::default(),
    }
}

/// 走**命令入口**测一次全文搜索：返回（毫秒, 命中总数）。
fn d25_search_once(state: &AppState, query: &str) -> (u128, i64) {
    let started = Instant::now();
    let page = search_fulltext_impl(state, d25_search_args(query)).expect("全文搜索");
    (started.elapsed().as_millis(), page.total)
}

/// **NFR-PERF-04（后端部分）**：全文搜索响应。
///
/// 口径（PRD §6.1.4）：起点 = 输入停顿（防抖结束）；终点 = 结果呈现。
/// 这里测的是**后端部分** —— 命令层 `search_fulltext_impl` 的耗时（含查询构造、FTS 检索、排序、
/// 片段定位与命中笔记读盘）；**不含** IPC 往返与前端渲染 —— 「到结果呈现」的后半跳仍属端到端/手工验收
/// （边界写在 `perf/d25-budgets.json` 的 `owning` 里）。
///
/// 两条查询：`ms` = 宽查询（命中全部笔记 ⇒ 走满一页并读 20 篇原文取片段，**最重**的常规情形）；
/// `ms_narrow` = 窄查询（确定性数字词，命中 0~1 篇）。先做一次不计时的热身，排除一次性初始化。
#[test]
fn d25_nfr_perf_04_search() {
    let notes_arg = d25_notes_arg();
    let (guard, root) = d25_vault(notes_arg);
    let (db_guard, pool, state) = d25_index_and_state(&root);
    let notes = d25_note_count(&pool);

    let narrow = format!("{}", 131 * (notes_arg / 2) + 5); // 开发夹具里必然存在的数字词
    let _ = d25_search_once(&state, "热身"); // 不计时：排除一次性初始化
    let (ms, hits) = d25_search_once(&state, "内容");
    let (ms_narrow, hits_narrow) = d25_search_once(&state, &narrow);
    let _ = (&guard, &db_guard);
    println!(
        "D25 NFR-PERF-04-search ms={ms} ms_narrow={ms_narrow} hits={hits} hits_narrow={hits_narrow} notes={notes} scope=backend query_wide=内容"
    );
    assert!(hits > 0, "宽查询在夹具上必须命中，否则这个耗时没有意义");
}

/// **NFR-PERF-04-line-filter（`line:` 候选上限的标定）**。
///
/// 口径（PRD §6.1.4 + §5.3.4.3）：起点 = 命令入口 `search_fulltext_impl`；终点 = 返回第一页。
/// 查询取**最坏情况**：只用 `line:(...)`、不带关键词与 path:/tag: 限定 ⇒ 候选 = 全部笔记。
///
/// 两条路径如实标出用的是哪条：
///  · 候选数 ≤ 上限 ⇒ 走**命令入口**（`path=command`，与 scope 一致）；
///  · 候选数 > 上限 ⇒ 命令入口会按 §5.3.4.3 拒绝（打印 `cmd_rejected=true`），此时改测
///    **同一组装路径**（`page_with_line_filter`，上限临时放开）以取得真实代价（`path=page-uncapped`）
///    —— 那是标定用的观测值，**不是**产品行为。
#[test]
fn d25_nfr_perf_04_line_filter() {
    let notes_arg = d25_notes_arg();
    let (guard, root) = d25_vault(notes_arg);
    let (db_guard, pool, state) = d25_index_and_state(&root);
    let notes = d25_note_count(&pool);

    let query =
        std::env::var("KP_D25_LINE_QUERY").unwrap_or_else(|_| "line:(内容 笔记)".to_string());
    let parsed = parse_query(&query, MatchMode::All).expect("解析 line: 查询");
    let candidates = search_query::fulltext_scan(&pool, &parsed, SortOrder::Path)
        .expect("取候选")
        .len();
    let _ = d25_search_once(&state, "热身"); // 不计时

    if candidates <= MAX_LINE_FILTER_CANDIDATES {
        let started = Instant::now();
        let page = search_fulltext_impl(&state, d25_search_args(&query)).expect("line: 搜索");
        let ms = started.elapsed().as_millis();
        println!(
            "D25 NFR-PERF-04-line-filter ms={ms} candidates={candidates} notes={notes} hits={} cap={MAX_LINE_FILTER_CANDIDATES} path=command scope=backend",
            page.total
        );
    } else {
        let cmd_rejected = search_fulltext_impl(&state, d25_search_args(&query)).is_err();
        let started = Instant::now();
        let page = page_with_line_filter(
            &pool,
            &root,
            &parsed,
            SortOrder::Path,
            ViewOptions {
                context_lines: 2,
                max_snippets: 5,
            },
            PageWindow {
                offset: 0,
                limit: 20,
            },
            usize::MAX,
        )
        .expect("不受上限约束的组装路径");
        let ms = started.elapsed().as_millis();
        println!(
            "D25 NFR-PERF-04-line-filter ms={ms} candidates={candidates} notes={notes} hits={} cap={MAX_LINE_FILTER_CANDIDATES} path=page-uncapped cmd_rejected={cmd_rejected} scope=backend",
            page.total
        );
    }
    let _ = (&guard, &db_guard);
}

/// 按**给定正文**写一篇笔记（AC-SEARCH-01 的夹具要精确控制哪些笔记含目标短语）。
fn write_note_body(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).expect("建目录");
    }
    fs::write(p, body).expect("写笔记");
}

/// **AC-SEARCH-01（严格阈值，PRD §4.5.2）**：Vault 中 3 篇正文含「知识图谱的力导向布局」，
/// 另有只含**片段词之一**（力导向 / 布局）的干扰笔记；搜「力导向布局」⇒
/// ① 3 篇全中（干扰项不得混入）；② 排在前列；③ 片段里「力导向布局」被高亮；④ 响应 < 200 ms。
///
/// 规模缺省按 PRD 取 **10000** 篇，可用 `KP_AC_SEARCH01_NOTES` 覆盖（CI 用得起的小规模）；
/// **四条判据一条都不随规模放宽**。打印 `D25 AC-SEARCH-01-04-strict ok=…` 供采样器判读。
#[test]
fn d25_ac_search_01_strict_chinese_search() {
    let notes: usize = std::env::var("KP_AC_SEARCH01_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    const PHRASE: &str = "知识图谱的力导向布局";
    let mut expected: Vec<String> = Vec::new();
    for i in 0..notes {
        let rel = format!("d{}/note-{i}.md", i % 50);
        let body = match i {
            0..=2 => {
                expected.push(rel.clone());
                format!("# 笔记 {i}\n\n{PHRASE} 很关键\n")
            }
            3..=7 => format!("# 笔记 {i}\n\n只讲 力导向 这一个词\n"),
            8..=12 => format!("# 笔记 {i}\n\n只讲 布局 这一个词\n"),
            _ => format!("# 笔记 {i}\n\n与本主题无关的内容 {i}\n"),
        };
        write_note_body(&root, &rel, &body);
    }
    let (db_guard, _pool, state) = d25_index_and_state(&root);

    let started = Instant::now();
    let page = search_fulltext_impl(&state, d25_search_args("力导向布局")).expect("搜索");
    let ms = started.elapsed().as_millis();

    // ① 恰好 3 篇：只含单个片段词的干扰笔记**不得**混进来
    let got: Vec<String> = page.items.iter().map(|i| i.rel_path.clone()).collect();
    assert_eq!(
        page.total, 3,
        "必须恰好命中 3 篇（干扰项不得混入），实得：{got:?}"
    );
    // ② 排在前列：total = 3 ⇒ 本页给出的前三条就是它们
    assert_eq!(page.items.len(), 3, "本页应给出这 3 条：{got:?}");
    for rel in &expected {
        assert!(got.contains(rel), "缺少命中：{rel} / 实得 {got:?}");
    }
    // ③ 片段高亮：高亮区间必须正好覆盖整条短语（FR-SEARCH-04）
    let first = page.items.first().expect("应有结果");
    let snippet = first.snippets.first().expect("应带片段");
    let line = snippet
        .lines
        .iter()
        .find(|l| l.line == snippet.highlight_line)
        .expect("高亮行必须在片段里");
    let marked: String = line
        .text
        .chars()
        .skip(snippet.highlight_start)
        .take(snippet.highlight_end - snippet.highlight_start)
        .collect();
    assert_eq!(marked, "力导向布局", "片段必须高亮命中的短语：{snippet:?}");
    // ④ 严格阈值
    assert!(
        ms < 200,
        "PRD §4.5.2：响应须 < 200 ms，实测 {ms} ms（notes={notes}）"
    );
    let _ = db_guard;
    println!(
        "D25 AC-SEARCH-01-04-strict ok=1 ms={ms} notes={notes} total={} highlighted={marked}",
        page.total
    );
}
