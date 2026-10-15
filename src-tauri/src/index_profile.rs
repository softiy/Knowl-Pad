//! 索引的**分项计时**（M5 批次 5 定位 NFR-PERF-03 用）。
//!
//! **只测量，不改行为**：不调整批大小、不动事务边界、不改索引语义。缺省**关闭**；
//! 关闭时每个插桩点只做一次 relaxed 原子读（纳秒级），对索引耗时可忽略。
//!
//! 为什么单独成文件：`index_engine.rs` 已是 CODE-11 的预警文件（>200 SLOC），
//! 把计数器与阶段枚举塞进去会把它推到硬上限附近。
//!
//! 用法：
//! ```ignore
//! reset(); enable(true);
//! let outcome = full_index_cancellable(...)?;   // 正常跑，不改任何参数
//! enable(false);
//! println!("{:?}", all_ms());                    // [(阶段名, 毫秒), ...]
//! ```
//!
//! **口径**：阶段耗时是**串行累加**的墙钟时间（各阶段互不重叠，唯一的例外是
//! `resolve` 覆盖它内部的 `resolve_links` / `recount_tags` / `txn_commit`）。
//! 各阶段之和**不等于**总耗时：差额是分批循环本身的编排开销（channel 往返、
//! `chunks()` 遍历、进度回调），这正好让"还有多少没被归因"可被审计。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// 可计时的阶段（判别值即 `ACC` 的下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Phase {
    /// 目录遍历 + stat（`scan_vault`）
    Scan = 0,
    /// `std::fs::read` 读文件内容
    Read = 1,
    /// Markdown 解析（`md_parse::parse`）
    Parse = 2,
    /// 分词（`tokenize::for_index`）
    Tokenize = 3,
    /// 内容指纹（FNV-1a）
    Fingerprint = 4,
    /// 清理旧行（note_fts / file）
    DeleteOld = 5,
    /// 写 `file` 行
    FileRow = 6,
    /// 写 `note_fts` 行
    Fts = 7,
    /// 派生表写入合计（heading + block_id + link + tag + file_alias）
    Derived = 8,
    /// `heading` 表
    Heading = 9,
    /// `block_id` 表
    BlockId = 10,
    /// `link` 表
    Link = 11,
    /// `tag` + `file_tag` 表
    Tag = 12,
    /// `file_alias` 表
    Alias = 13,
    /// 分批事务的 `BEGIN IMMEDIATE`
    TxnBegin = 14,
    /// 所有 `COMMIT`（含 fsync）
    TxnCommit = 15,
    /// 收尾裁决阶段合计（含它内部的子阶段与提交）
    Resolve = 16,
    /// `resolve_links`
    ResolveLinks = 17,
    /// `recount_tags`
    RecountTags = 18,
}

const PHASE_COUNT: usize = 19;

/// 阶段名（下标与 `Phase` 的判别值一致）。
pub const PHASE_NAMES: [&str; PHASE_COUNT] = [
    "scan",
    "read",
    "parse",
    "tokenize",
    "fingerprint",
    "delete_old",
    "file_row",
    "fts",
    "derived",
    "heading",
    "block_id",
    "link",
    "tag",
    "alias",
    "txn_begin",
    "txn_commit",
    "resolve",
    "resolve_links",
    "recount_tags",
];

/// 计数器是**进程级全局状态**（写入发生在数据库写线程上，不能改成 thread-local）。
/// 代价是用例之间会互相影响：凡是要"看数字"的用例都必须先持有这把锁，
/// 否则并行跑时会把别的用例的索引工作也统计进来。
#[cfg(test)]
pub static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

static ENABLED: AtomicBool = AtomicBool::new(false);
static ACC: [AtomicU64; PHASE_COUNT] = [const { AtomicU64::new(0) }; PHASE_COUNT];
static FILES: AtomicU64 = AtomicU64::new(0);

/// 计时是否开启（插桩点先读它，关闭时不做任何 `Instant::now()`）。
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 开关计时。
pub fn enable(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// 清零所有计数器。
pub fn reset() {
    for slot in &ACC {
        slot.store(0, Ordering::Relaxed);
    }
    FILES.store(0, Ordering::Relaxed);
}

/// 记一个"被索引的文件"（分母：ms/篇 用）。
pub fn count_file() {
    if enabled() {
        FILES.fetch_add(1, Ordering::Relaxed);
    }
}

/// 已计数的文件数。
pub fn files() -> u64 {
    FILES.load(Ordering::Relaxed)
}

/// 开始一段计时；**关闭时返回 None**，调用方照常执行（不做 `Instant::now()`）。
pub fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// 结束一段计时并累加到该阶段。
pub fn stop(token: Option<Instant>, phase: Phase) {
    if let Some(t) = token {
        add(phase, t.elapsed());
    }
}

/// 直接累加一段已知时长（用于无法用 token 包裹的场合）。
///
/// **同样受开关约束**：关闭时不记录 —— 否则"关闭即不测量"的契约就有一条缝。
pub fn add(phase: Phase, spent: Duration) {
    if enabled() {
        ACC[phase as usize].fetch_add(spent.as_nanos() as u64, Ordering::Relaxed);
    }
}

/// 某阶段的累计耗时（毫秒，截断）。
pub fn ms(phase: Phase) -> u128 {
    ACC[phase as usize].load(Ordering::Relaxed) as u128 / 1_000_000
}

/// 全部阶段的（名, 毫秒）：供一行打印与表格化。
pub fn all_ms() -> Vec<(&'static str, u128)> {
    PHASE_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| (*name, ACC[i].load(Ordering::Relaxed) as u128 / 1_000_000))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取测试锁（中毒也继续：它保护的是"数字别串台"，不是不变量）。
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 缺省关闭：插桩不得在没人要求时计时（否则每个索引点都白付一次 now()）。
    #[test]
    fn disabled_by_default_and_start_is_free() {
        let _guard = lock();
        enable(false);
        reset();
        assert!(!enabled(), "缺省必须关闭");
        assert!(start().is_none(), "关闭时 start() 返回 None");
    }

    /// 开关 + 累加 + 清零的语义。
    #[test]
    fn enable_accumulate_and_reset() {
        let _guard = lock();
        enable(true);
        assert!(start().is_some());
        add(Phase::Parse, Duration::from_millis(7));
        add(Phase::Parse, Duration::from_millis(5));
        assert_eq!(ms(Phase::Parse), 12, "同阶段累加");
        assert_eq!(ms(Phase::Scan), 0, "未计的阶段为 0");
        count_file();
        count_file();
        assert_eq!(files(), 2, "文件计数是 ms/篇 的分母");
        reset();
        assert_eq!(ms(Phase::Parse), 0);
        assert_eq!(files(), 0);
        enable(false);
        add(Phase::Parse, Duration::from_millis(3));
        assert_eq!(ms(Phase::Parse), 0, "关闭后不再累加");
    }

    /// 阶段名表与枚举必须一一对应（下标即判别值）。
    #[test]
    fn phase_names_match_discriminants() {
        let _guard = lock();
        assert_eq!(PHASE_NAMES.len(), PHASE_COUNT);
        assert_eq!(PHASE_NAMES[Phase::Scan as usize], "scan");
        assert_eq!(PHASE_NAMES[Phase::TxnCommit as usize], "txn_commit");
        assert_eq!(PHASE_NAMES[Phase::RecountTags as usize], "recount_tags");
        assert_eq!(all_ms().len(), PHASE_COUNT);
    }
}
