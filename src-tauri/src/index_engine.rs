//! 全量索引引擎（M3 WP4 第一切片：**遍历 → 解析 → 分词 → 批量写库**）。
//!
//! 边界说明：
//! - **解析与分词在 `kp-domain`**（纯逻辑，可独立单测）；本模块负责 IO 与 SQL 编排；
//! - **链接裁决**（resolved/dangling/ambiguous）是独立阶段：本切片统一先写 `status = dangling`，
//!   裁决与标签计数重算留待下一切片（勘误 D-20 明确裁决属 M3 索引阶段）；
//! - 遍历与（后续的）文件监听**必须复用同一份忽略规则**（技术方案 TECH:1752-1754）。

// 本切片的引擎尚未接线（遍历命令与事件在 PR-4 下一切片）；接线后删除本行。

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};

use kp_domain::error::AppError;
use kp_domain::md_parse;
use kp_domain::tokenize;

use crate::index_sql;
use crate::index_write::insert_note_derived;
use crate::storage::pool::DbPool;

// ── 扫描侧已拆到 index_scan.rs（CODE-11 硬上限）；这里重新导出，既有调用路径不变 ──
pub use crate::index_scan::{
    content_fingerprint, is_ignored_dir, scan_vault, ScanEntry, IGNORED_DIRS,
};
use crate::index_scan::{kind_of, mtime_ms, split_name};

/// 每批提交的文件数（PERF-05：禁止逐条事务）。
pub const BATCH_SIZE: usize = 200;

/// 单文件解析超时（SEC-03「单文件解析设置大小上限（默认 5MB）与超时保护（默认 10s），超限跳过并记录警告」）。
///
/// 为什么是**跳过该文件**而不是中断整次索引：一个特制 Markdown 不该让整个 Vault 索引不出来
/// （与扫描侧 `MAX_NOTE_BYTES` 的 `skip-oversize:` 同口径）。实现在 `parse_with_timeout`。
pub const PARSE_TIMEOUT: Duration = Duration::from_secs(10);

/// 单个文件的索引结果（`index_file_mode` 的返回）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    /// 已写入（重建时含先删旧行再写新行）
    Indexed,
    /// 按 `IndexMode::SkipUnchanged` 跳过（mtime / 大小 / 内容指纹都未变）
    Skipped,
    /// 解析超时（SEC-03）：**一行都没写**，调用方记一条警告
    TimedOut,
}

/// 全量索引的结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexOutcome {
    pub indexed: usize,
    pub skipped: usize,
    /// 是否因取消请求提前结束（AC-VAULT-05）
    pub cancelled: bool,
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

/// 跳过策略：Force 一律重建；SkipUnchanged 时"**未变**"的文件跳过。
///
/// **「未变」= `mtime` + 大小 + 内容指纹三者都相同**（DEBT-35，2026-10-17 批次 4 起；此前只看前两者，
/// 于是"内容变了但 mtime 与大小都没变"（脚本按固定时间戳改写、等长替换）会被漏掉，索引静默停在旧解析结果上）。
/// 代价是**未变的笔记也要读一遍盘**才能算指纹 —— 省下的是解析 / 分词 / 写库，不是读盘；这条边界见 `is_unchanged`。
/// 此外 index_rebuild(force = true) 始终提供一条不看任何判据的可靠全量路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Force,
    SkipUnchanged,
}

#[allow(dead_code)]
pub fn index_file(conn: &Connection, root: &Path, entry: &ScanEntry) -> Result<(), AppError> {
    index_file_mode(conn, root, entry, IndexMode::Force).map(|_| ())
}

/// 同上，但可按 IndexMode 跳过未变更文件、并在解析超时时跳过该文件（见 [`FileOutcome`]）。
pub fn index_file_mode(
    conn: &Connection,
    root: &Path,
    entry: &ScanEntry,
    mode: IndexMode,
) -> Result<FileOutcome, AppError> {
    index_file_mode_with_timeout(conn, root, entry, mode, PARSE_TIMEOUT)
}

/// 同上，但把单文件解析超时**作为参数**：生产路径固定走 [`PARSE_TIMEOUT`]，用例注入
/// `Duration::ZERO` 来确定性地打中"超时 ⇒ 跳过且不留半成品"这条分支（真的等 10 秒的用例
/// 不会有人跑，而超时的**后果**与"等了多久"无关）。
pub(crate) fn index_file_mode_with_timeout(
    conn: &Connection,
    root: &Path,
    entry: &ScanEntry,
    mode: IndexMode,
    parse_timeout: Duration,
) -> Result<FileOutcome, AppError> {
    use crate::index_profile::{start, stop, Phase};
    crate::index_profile::count_file();
    let abs = root.join(&entry.rel_path);
    let kind = kind_of(&entry.rel_path);
    let is_note = kind == "note";
    let t = start();
    let bytes = std::fs::read(&abs).map_err(AppError::from)?;
    stop(t, Phase::Read);
    let t = start();
    let fingerprint = content_fingerprint(&bytes);
    stop(t, Phase::Fingerprint);
    // DEBT-35：判定必须在**读到内容之后**做 —— 指纹只能从内容算出来。
    // 笔记比指纹（它的内容会进索引）；附件只比 mtime + 大小（内容不进索引，读它纯属浪费）。
    if mode == IndexMode::SkipUnchanged && is_unchanged(conn, entry, &fingerprint, is_note)? {
        return Ok(FileOutcome::Skipped);
    }
    let note = if is_note {
        let t = start();
        let parsed = match parse_with_timeout(&bytes, parse_timeout) {
            Ok(parsed) => parsed,
            Err(_) => {
                stop(t, Phase::Parse);
                // SEC-03：超时 ⇒ 跳过该文件并记警告。此处**还没有写过任何行**（删旧行在解析之后），
                // 所以不会留下"半成品" —— `timed_out_file_is_skipped_without_partial_rows` 钉住它。
                tracing::warn!(
                    rel_path = %entry.rel_path,
                    timeout_secs = parse_timeout.as_secs(),
                    "单文件解析超时，已跳过（SEC-03 / DEBT-32）"
                );
                return Ok(FileOutcome::TimedOut);
            }
        };
        stop(t, Phase::Parse);
        Some(parsed)
    } else {
        None
    };
    let indexed_text = if let Some(parsed) = note.as_ref() {
        let t = start();
        let text = tokenize::for_index(&parsed.plain_text);
        stop(t, Phase::Tokenize);
        text
    } else {
        String::new()
    };
    let now = mtime_ms(&abs).max(1);

    delete_old_rows(conn, &entry.rel_path)?;
    let file_id = insert_file_row(conn, entry, kind, &fingerprint, now)?;

    if is_note {
        let t = start();
        let mut stmt = conn
            .prepare_cached(index_sql::FTS_INSERT_SQL)
            .map_err(|e| AppError::db_with("准备写入全文索引", &e))?;
        stmt.execute(params![file_id, indexed_text])
            .map_err(|e| AppError::db_with("写入全文索引", &e))?;
        stop(t, Phase::Fts);
    }

    let Some(note) = note.as_ref() else {
        return Ok(FileOutcome::Indexed);
    };
    let t = start();
    insert_note_derived(conn, file_id, note)?;
    stop(t, Phase::Derived);
    Ok(FileOutcome::Indexed)
}

/// 「未变」判定（**DEBT-35**，2026-10-17 批次 4）：`mtime` + 大小 + **内容指纹**三者都相同才算未变。
///
/// 为什么必须比指纹：只看 `mtime + size` 时，"脚本按固定时间戳改写 / 等长替换"这类改动会被判成未变，
/// 索引于是静默停在旧解析结果上（搜索、反链、标签都指向旧内容，界面上却一切正常）。
///
/// **为什么 `check_hash` 是可选的（笔记比、附件不比）**：指纹只能从**内容**算出来，所以比指纹就意味着读盘
/// —— 而只有笔记的内容会进索引（附件只登记元信息）。让附件也比指纹，等于每次增量都把整个 Vault 的
/// 二进制读一遍，却换不来任何正确性。这条边界是刻意的，不是遗漏（用例 `attachment_skip_still_uses_mtime_and_size`
/// 把它钉住）。
fn is_unchanged(
    conn: &Connection,
    entry: &ScanEntry,
    fingerprint: &str,
    check_hash: bool,
) -> Result<bool, AppError> {
    use rusqlite::OptionalExtension;
    let stored: Option<(i64, i64, Option<String>)> = conn
        .query_row(
            "SELECT mtime_ms, size_bytes, content_hash FROM file WHERE rel_path = ?1 AND deleted = 0",
            params![entry.rel_path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| AppError::db_with("读取未变判定", &e))?;
    let Some((mtime_ms, size_bytes, content_hash)) = stored else {
        return Ok(false);
    };
    if mtime_ms != entry.mtime_ms || size_bytes != entry.size_bytes as i64 {
        return Ok(false);
    }
    if !check_hash {
        return Ok(true);
    }
    // 旧库（本列加入之前写的行）可能是 NULL —— NULL ≠ 指纹，照"未变"处理会让 DEBT-35 的缺口复活。
    Ok(content_hash.as_deref() == Some(fingerprint))
}

/// 删掉该路径的旧行（先 `note_fts` 再 `file`）。
///
/// 顺序不可颠倒：`note_fts` 的 rowid 与 `file.id` 对齐，**必须先取到 id 再删 file 行**。
/// 抽成函数只是为压 CODE-11 的函数级行数上限（clippy::too_many_lines），语义未变。
fn delete_old_rows(conn: &Connection, rel_path: &str) -> Result<(), AppError> {
    use crate::index_profile::{start, stop, Phase};
    let t = start();
    let old_id: Option<i64> = {
        let mut stmt = conn
            .prepare_cached(index_sql::OLD_FILE_ID_SQL)
            .map_err(|e| AppError::db_with("准备读取旧文件行", &e))?;
        stmt.query_row(params![rel_path], |r| r.get(0)).ok()
    };
    if let Some(id) = old_id {
        let mut stmt = conn
            .prepare_cached(index_sql::FTS_DELETE_SQL)
            .map_err(|e| AppError::db_with("准备清理旧 FTS 行", &e))?;
        stmt.execute(params![id])
            .map_err(|e| AppError::db_with("清理旧 FTS 行", &e))?;
        let mut stmt = conn
            .prepare_cached(index_sql::FILE_DELETE_SQL)
            .map_err(|e| AppError::db_with("准备清理旧文件行", &e))?;
        stmt.execute(params![id])
            .map_err(|e| AppError::db_with("清理旧文件行", &e))?;
    }
    stop(t, Phase::DeleteOld);
    Ok(())
}

/// 写 `file` 行并返回新 id（同事务内后续的派生行都挂在它下面）。
fn insert_file_row(
    conn: &Connection,
    entry: &ScanEntry,
    kind: &str,
    fingerprint: &str,
    now: i64,
) -> Result<i64, AppError> {
    use crate::index_profile::{start, stop, Phase};
    let (name, stem, ext) = split_name(&entry.rel_path);
    let t = start();
    {
        let mut stmt = conn
            .prepare_cached(index_sql::FILE_INSERT_SQL)
            .map_err(|e| AppError::db_with("准备写入文件行", &e))?;
        stmt.execute(params![
            entry.rel_path,
            name,
            stem,
            ext,
            kind,
            entry.size_bytes as i64,
            entry.mtime_ms,
            fingerprint,
            now
        ])
        .map_err(|e| AppError::db_with("写入文件行", &e))?;
    }
    let file_id = conn.last_insert_rowid();
    stop(t, Phase::FileRow);
    Ok(file_id)
}

/// 带超时的解析（SEC-03）：把"是否该停"做成**截止时间判据**交给解析器（协作式中止）。
///
/// 抽成函数有两个理由：① 生产路径与用例走同一条代码；② 用例可以传 `Duration::ZERO`
/// 确定性地打中"已超时"分支（否则只能真的等 10 秒 —— 那种用例不会有人跑）。
fn parse_with_timeout(
    bytes: &[u8],
    timeout: Duration,
) -> Result<md_parse::ParsedNote, md_parse::ParseAborted> {
    let deadline = Instant::now() + timeout;
    md_parse::parse_with_guard(bytes, &|| Instant::now() >= deadline)
}

/// 全量索引（默认 Force；不检查取消）。生产走 `full_index_cancellable`，这里是测试用包装。
#[allow(dead_code)]
pub fn full_index<F: FnMut(usize, usize)>(
    pool: &DbPool,
    root: &Path,
    progress: F,
) -> Result<IndexOutcome, AppError> {
    full_index_cancellable(pool, root, progress, IndexMode::Force, || false)
}

/// 全量索引：分批事务写库、按批回调进度、批间检查取消（AC-VAULT-05）。
pub fn full_index_cancellable<F: FnMut(usize, usize), C: Fn() -> bool>(
    pool: &DbPool,
    root: &Path,
    progress: F,
    mode: IndexMode,
    should_cancel: C,
) -> Result<IndexOutcome, AppError> {
    full_index_with_timeout(pool, root, progress, mode, should_cancel, PARSE_TIMEOUT)
}

/// 同上，但单文件解析超时**可注入**（SEC-03 的默认值是 [`PARSE_TIMEOUT`]）。
///
/// 用例注入零超时来钉住"超时 ⇒ 跳过该文件 + `skip-timeout:` 警告 + 一行都不写"这条完整通路；
/// 生产路径只经 [`full_index_cancellable`]，因此超时值仍然只有一处（[`PARSE_TIMEOUT`]）。
pub fn full_index_with_timeout<F: FnMut(usize, usize), C: Fn() -> bool>(
    pool: &DbPool,
    root: &Path,
    mut progress: F,
    mode: IndexMode,
    should_cancel: C,
    parse_timeout: Duration,
) -> Result<IndexOutcome, AppError> {
    let started = Instant::now();
    let (entries, warnings) = scan_vault(root)?;
    let total = entries.len();
    let mut outcome = IndexOutcome {
        warnings,
        ..Default::default()
    };
    let mut done = 0usize;
    for chunk in entries.chunks(BATCH_SIZE) {
        if should_cancel() {
            outcome.cancelled = true;
            outcome.duration_ms = started.elapsed().as_millis() as u64;
            return Ok(outcome);
        }
        let chunk: Vec<ScanEntry> = chunk.to_vec();
        let chunk_len = chunk.len();
        let root_owned = root.to_path_buf();
        // 闭包返回 (写入数, 超时文件清单)：超时清单必须带出事务边界之外再进 outcome.warnings。
        let (indexed, timed_out) = pool.with_writer(move |conn| {
            use crate::index_profile::{start, stop, Phase};
            let t = start();
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| AppError::db_with("开启索引事务", &e))?;
            stop(t, Phase::TxnBegin);
            let mut n = 0usize;
            let mut timed_out: Vec<String> = Vec::new();
            for entry in &chunk {
                match index_file_mode_with_timeout(conn, &root_owned, entry, mode, parse_timeout)? {
                    FileOutcome::Indexed => n += 1,
                    FileOutcome::TimedOut => {
                        timed_out.push(format!("skip-timeout:{}", entry.rel_path));
                    }
                    FileOutcome::Skipped => {}
                }
            }
            let t = start();
            conn.execute_batch("COMMIT")
                .map_err(|e| AppError::db_with("提交索引事务", &e))?;
            stop(t, Phase::TxnCommit);
            Ok((n, timed_out))
        })?;
        outcome.indexed += indexed;
        // 超时文件既不计入 indexed 也不计入 skipped：它只留一条可诊断的警告
        // （命名与扫描侧的 skip-oversize: 对齐，前端状态栏与日志都按同一套前缀识别）。
        outcome.warnings.extend(timed_out);
        done += chunk_len;
        progress(done, total);
    }
    // 全部文件写完后统一裁决（此时候选行才齐）；裁决与计数在同一事务内完成
    pool.with_writer(|conn| {
        use crate::index_profile::{start, stop, Phase};
        let t_all = start();
        let t = start();
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AppError::db_with("开启裁决事务", &e))?;
        stop(t, Phase::TxnBegin);
        let t = start();
        crate::index_resolve::resolve_links(conn)?;
        stop(t, Phase::ResolveLinks);
        let t = start();
        crate::index_resolve::recount_tags(conn)?;
        stop(t, Phase::RecountTags);
        let t = start();
        conn.execute_batch("COMMIT")
            .map_err(|e| AppError::db_with("提交裁决事务", &e))?;
        stop(t, Phase::TxnCommit);
        stop(t_all, Phase::Resolve);
        Ok(())
    })?;
    // 收尾：清理**孤儿标签行**（AC-TAG-02 末句「旧标签若无其他子孙则从标签表消失」；
    // 重建与一次性索引必须**逐表一致** —— AC-REL-03 口径。见 PRD 勘误 D-41。）
    // 只在这里做（未取消的全量重建收尾）：中途取消在上面的 should_cancel 分支已提前返回，
    // 而每批中间不能删 —— 那时某些标签可能还没轮到它的文件。
    pool.with_writer(|conn| {
        conn.execute(crate::index_sql::ORPHAN_TAGS_DELETE_SQL, [])
            .map(|_| ())
            .map_err(|e| AppError::db_with("清理孤儿标签", &e))
    })?;
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    Ok(outcome)
}
