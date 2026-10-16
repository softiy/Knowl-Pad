//! **机器校准微基准**（跨机器可比、零新依赖、**只测量不改行为**）。
//!
//! 为什么需要它：PRD §6.1.1 的基准环境是「4 核 @2.5GHz / 8GB / SATA SSD」，而开发机与 CI runner
//! 都不是基准机。光比较"首次索引多少毫秒"无法判断差异里有多少来自机器 —— 这两个标量把机器差异
//! 压成**单核吞吐**与**持久化延迟**两个维度，从而能把某台机器上的索引耗时往基准环境量级上做
//! **有假设的换算**（换算的假设与区间必须一起写出来，见 M5 计划 §7.1）。
//!
//! 跑法（任意机器原样可跑，无需夹具）：
//! ```text
//! cargo test -p knowl-pad --locked calib_ -- --nocapture --test-threads=1
//! ```
//! 输出（越小越快）：
//! ```text
//! D25 CALIB-cpu us_per_iter=… median_us=… min_us=… max_us=… rounds=… iters_per_round=… note_bytes=…
//! D25 CALIB-persist us_per_commit=… median_commit_us=… … checkpoint_us=… rounds=… journal_mode=… synchronous=…
//! ```

use std::time::Instant;

use rusqlite::Connection;

use kp_domain::{md_parse, tokenize};

use crate::storage::pragma;

/// 固定输入：与标准档夹具同形状（frontmatter + 标题 + 正文 + 链接 + 标签），约 2 KB。
///
/// **必须是固定字符串**：校准量的意义是"同一份工作在任意机器上多快"，输入一变就不可比。
fn fixed_note() -> String {
    let mut s = String::from("---\ntags: [项目/甲, 性能]\naliases: [校准样例]\n---\n# 标题甲\n\n");
    for i in 0..18 {
        s.push_str(&format!(
            "第 {i} 段中文正文，包含 [[目标笔记|别名{i}]] 与 #标签{i} 以及行内代码 `code{i}`。\
             这一段刻意混入 English words and numbers 12345 来模拟真实笔记的词元分布。\n\n"
        ));
    }
    s
}

/// 中位数（排序后取中间；样本数为奇数或偶数都取 len/2，口径写死便于跨机器比较）。
fn median(v: &[u128]) -> u128 {
    let mut s = v.to_vec();
    s.sort_unstable();
    s[s.len() / 2]
}

fn min_max(v: &[u128]) -> (u128, u128) {
    (
        *v.iter().min().expect("非空"),
        *v.iter().max().expect("非空"),
    )
}

/// **单核吞吐**：固定笔记上反复跑"解析 + 分词" —— 索引路径里最重的两段纯 CPU 工作
/// （与 `index_engine` 的第 87/95 行完全同形：`md_parse::parse(bytes)` → `tokenize::for_index(plain_text)`）。
///
/// 输出 `us_per_iter`＝每轮迭代的平均微秒数；同时给多轮的中位数与极差（运行间波动）。
#[test]
fn calib_cpu_parse_and_tokenize() {
    const ITERS: usize = 200;
    const ROUNDS: usize = 5;
    let text = fixed_note();
    let bytes = text.as_bytes();
    // 词典加载是一次性成本（应用启动时已预热），**不计入吞吐**
    tokenize::warm_up();
    for _ in 0..20 {
        let p = md_parse::parse(bytes);
        let _ = tokenize::for_index(&p.plain_text);
    }
    let mut per = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let t = Instant::now();
        for _ in 0..ITERS {
            let p = md_parse::parse(bytes);
            let _ = tokenize::for_index(&p.plain_text);
        }
        per.push(t.elapsed().as_micros() / ITERS as u128);
    }
    let (lo, hi) = min_max(&per);
    println!(
        "D25 CALIB-cpu us_per_iter={} median_us={} min_us={lo} max_us={hi} rounds={ROUNDS} iters_per_round={ITERS} note_bytes={}",
        median(&per),
        median(&per),
        bytes.len()
    );
}

/// **持久化延迟**：与生产**同一套 PRAGMA**（WAL + synchronous=NORMAL + wal_autocheckpoint=1000 等，
/// 见 `storage::pragma`），在临时库上做 M 次"小事务提交"取中位数；
/// 另测一次 `wal_checkpoint(TRUNCATE)`（先写 ~4MB WAL）—— 后者与磁盘延迟强相关，
/// 也正是分批提交真正花掉的那部分（本机实测每个 chunk 的 COMMIT 约 32 ms，量级上更接近 checkpoint 而非 WAL 追加）。
///
/// 输出里同时打印生效的 `journal_mode` / `synchronous`：读数字的人必须知道"这些提交到底 fsync 了没有"。
#[test]
fn calib_persist_commit_and_checkpoint() {
    const COMMITS: usize = 100;
    const ROUNDS: usize = 5;
    let mut per_commit = Vec::with_capacity(ROUNDS);
    let mut per_ckpt = Vec::with_capacity(ROUNDS);
    let mut jm = String::new();
    let mut sync_level = 0i64;
    for _ in 0..ROUNDS {
        let dir = tempfile::tempdir().expect("临时目录");
        let conn = Connection::open(dir.path().join("calib.db")).expect("打开校准库");
        pragma::apply(&conn).expect("应用与生产一致的 PRAGMA");
        jm = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("journal_mode");
        sync_level = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .expect("synchronous");
        conn.execute_batch("CREATE TABLE calib(id INTEGER PRIMARY KEY, v TEXT)")
            .expect("建表");
        let t = Instant::now();
        for i in 0..COMMITS {
            conn.execute_batch("BEGIN IMMEDIATE").expect("begin");
            conn.execute(
                "INSERT INTO calib(v) VALUES (?1)",
                rusqlite::params![format!("v{i}")],
            )
            .expect("insert");
            conn.execute_batch("COMMIT").expect("commit");
        }
        per_commit.push(t.elapsed().as_micros() / COMMITS as u128);
        // 写 ~4MB WAL 再 checkpoint：这一段主要花在磁盘上，不是 CPU
        let payload = "x".repeat(4096);
        conn.execute_batch("BEGIN IMMEDIATE").expect("begin");
        {
            let mut st = conn
                .prepare("INSERT INTO calib(v) VALUES (?1)")
                .expect("prepare");
            for _ in 0..1000 {
                st.execute(rusqlite::params![payload]).expect("insert");
            }
        }
        conn.execute_batch("COMMIT").expect("commit");
        let t = Instant::now();
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        per_ckpt.push(t.elapsed().as_micros());
        drop(conn);
        drop(dir);
    }
    let (clo, chi) = min_max(&per_commit);
    let (klo, khi) = min_max(&per_ckpt);
    println!(
        "D25 CALIB-persist us_per_commit={} median_commit_us={} min_commit_us={clo} max_commit_us={chi} \
         checkpoint_us={} min_checkpoint_us={klo} max_checkpoint_us={khi} rounds={ROUNDS} commits_per_round={COMMITS} \
         journal_mode={jm} synchronous={sync_level}",
        median(&per_commit),
        median(&per_commit),
        median(&per_ckpt)
    );
}
