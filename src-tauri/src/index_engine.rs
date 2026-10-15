//! 全量索引引擎（M3 WP4 第一切片：**遍历 → 解析 → 分词 → 批量写库**）。
//!
//! 边界说明：
//! - **解析与分词在 `kp-domain`**（纯逻辑，可独立单测）；本模块负责 IO 与 SQL 编排；
//! - **链接裁决**（resolved/dangling/ambiguous）是独立阶段：本切片统一先写 `status = dangling`，
//!   裁决与标签计数重算留待下一切片（勘误 D-20 明确裁决属 M3 索引阶段）；
//! - 遍历与（后续的）文件监听**必须复用同一份忽略规则**（技术方案 TECH:1752-1754）。

// 本切片的引擎尚未接线（遍历命令与事件在 PR-4 下一切片）；接线后删除本行。

use std::path::Path;
use std::time::Instant;

use rusqlite::{params, Connection};

use kp_domain::error::AppError;
use kp_domain::md_parse;
use kp_domain::tokenize;

use crate::storage::pool::DbPool;

// ── 扫描侧已拆到 index_scan.rs（CODE-11 硬上限）；这里重新导出，既有调用路径不变 ──
pub use crate::index_scan::{
    content_fingerprint, is_ignored_dir, scan_vault, ScanEntry, IGNORED_DIRS,
};
use crate::index_scan::{kind_of, mtime_ms, split_name};

/// 每批提交的文件数（PERF-05：禁止逐条事务）。
pub const BATCH_SIZE: usize = 200;

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

/// 跳过策略：Force 一律重建；SkipUnchanged 时 mtime 与大小都未变的文件直接跳过。
///
/// 说明：mtime + size 相同但内容被改的情况检测不到；此前注释写"待 PR-5 补"**已过期**（PR-5 只交付了增量索引本身），现登记为 DEBT-35。
/// 因此 index_rebuild(force = true) 始终提供一条可靠的全量路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Force,
    SkipUnchanged,
}

#[allow(dead_code)]
pub fn index_file(conn: &Connection, root: &Path, entry: &ScanEntry) -> Result<(), AppError> {
    index_file_mode(conn, root, entry, IndexMode::Force).map(|_| ())
}

/// 同上，但可按 IndexMode 跳过未变更文件（返回 false 表示跳过）。
pub fn index_file_mode(
    conn: &Connection,
    root: &Path,
    entry: &ScanEntry,
    mode: IndexMode,
) -> Result<bool, AppError> {
    if mode == IndexMode::SkipUnchanged {
        let unchanged: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM file WHERE rel_path = ?1 AND deleted = 0 AND mtime_ms = ?2 AND size_bytes = ?3",
                params![entry.rel_path, entry.mtime_ms, entry.size_bytes as i64],
                |r| r.get(0),
            )
            .ok();
        if unchanged.is_some() {
            return Ok(false);
        }
    }
    use crate::index_profile::{start, stop, Phase};
    crate::index_profile::count_file();
    let abs = root.join(&entry.rel_path);
    let kind = kind_of(&entry.rel_path);
    let is_note = kind == "note";
    let t = start();
    let bytes = std::fs::read(&abs).map_err(AppError::from)?;
    stop(t, Phase::Read);
    let note = if is_note {
        let t = start();
        let parsed = md_parse::parse(&bytes);
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
    let t = start();
    let fingerprint = content_fingerprint(&bytes);
    stop(t, Phase::Fingerprint);
    let now = mtime_ms(&abs).max(1);

    // 旧 rowid（note_fts 的 rowid 与 file.id 对齐）必须在删 file 之前取
    let t = start();
    let old_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM file WHERE rel_path = ?1",
            params![entry.rel_path],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = old_id {
        conn.execute("DELETE FROM note_fts WHERE rowid = ?1", params![id])
            .map_err(|e| AppError::db_with("清理旧 FTS 行", &e))?;
        conn.execute("DELETE FROM file WHERE id = ?1", params![id])
            .map_err(|e| AppError::db_with("清理旧文件行", &e))?;
    }
    stop(t, Phase::DeleteOld);

    let (name, stem, ext) = split_name(&entry.rel_path);
    let t = start();
    conn.execute(
        "INSERT INTO file (rel_path, name, stem, ext, kind, size_bytes, mtime_ms, content_hash, deleted, indexed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
        params![entry.rel_path, name, stem, ext, kind, entry.size_bytes as i64, entry.mtime_ms, fingerprint, now],
    )
    .map_err(|e| AppError::db_with("写入文件行", &e))?;
    let file_id = conn.last_insert_rowid();
    stop(t, Phase::FileRow);

    if is_note {
        let t = start();
        conn.execute(
            "INSERT INTO note_fts(rowid, plain_text) VALUES (?1, ?2)",
            params![file_id, indexed_text],
        )
        .map_err(|e| AppError::db_with("写入全文索引", &e))?;
        stop(t, Phase::Fts);
    }

    let Some(note) = note.as_ref() else {
        return Ok(true);
    };
    let t = start();
    insert_note_derived(conn, file_id, note)?;
    stop(t, Phase::Derived);
    Ok(true)
}

/// 标签写入用的三条 SQL（提到常量：`prepare_cached` 按 SQL 文本做键，文本必须逐字一致）。
const TAG_UPSERT_SQL: &str =
    "INSERT INTO tag (norm, display, depth, is_leaf, ref_count) VALUES (?1, ?2, ?3, ?4, 0) \
     ON CONFLICT(norm) DO UPDATE SET display = excluded.display, depth = excluded.depth, is_leaf = excluded.is_leaf";
const TAG_SELECT_ID_SQL: &str = "SELECT id FROM tag WHERE norm = ?1";
const FILE_TAG_INSERT_SQL: &str =
    "INSERT INTO file_tag (file_id, tag_id, line, col) VALUES (?1, ?2, ?3, ?4)";

/// 把解析结果写入派生表（标题 / 块 ID / 链接 / 标签 / 别名）。
///
/// 抽取自 `index_file_mode`：CODE-11 的函数级上限是 100 SLOC，原函数已到 113。
/// 边界取"事务内、依赖 file_id 的派生写入"这一整体 —— 拆在别处会把事务语义切碎。
fn insert_note_derived(
    conn: &Connection,
    file_id: i64,
    note: &md_parse::ParsedNote,
) -> Result<(), AppError> {
    use crate::index_profile::{start, stop, Phase};
    let t = start();
    for (i, h) in note.headings.iter().enumerate() {
        conn.execute(
        "INSERT INTO heading (file_id, level, text, anchor, line, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![file_id, h.level as i64, h.text, h.text, h.line as i64, i as i64],
    )
    .map_err(|e| AppError::db_with("写入标题", &e))?;
    }
    stop(t, Phase::Heading);
    let t = start();
    for b in &note.block_ids {
        conn.execute(
        "INSERT OR IGNORE INTO block_id (file_id, bid, line_start, line_end) VALUES (?1, ?2, ?3, ?4)",
        params![file_id, b.id, b.start_line as i64, b.end_line as i64],
    )
    .map_err(|e| AppError::db_with("写入块 ID", &e))?;
    }
    stop(t, Phase::BlockId);
    let t = start();
    for l in &note.links {
        let kind = match l.kind {
            md_parse::LinkKind::Embed => "embed",
            md_parse::LinkKind::Link => "link",
        };
        conn.execute(
        "INSERT INTO link (src_file_id, dst_file_id, target_ref, anchor, alias, link_kind, status, line, col) \
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, 'dangling', ?6, ?7)",
        params![file_id, l.target, l.anchor, l.alias, kind, l.line as i64, l.col as i64],
    )
    .map_err(|e| AppError::db_with("写入链接", &e))?;
    }
    stop(t, Phase::Link);
    // 注意：下面的循环变量历史上就叫 t —— 计时 token 改名以免与它混淆
    let t_tags = start();
    for t in &note.tags {
        let depth = t.name.split('/').count() as i64;
        // 两段式：先 upsert 标签行，再取回它的 id。
        //
        // **RETURNING 变体实测更慢，已回退**（2026-10-15，标准档 3000 篇、两轮对照）：
        // 把两条语句合成「INSERT … ON CONFLICT … DO UPDATE … RETURNING id」后，
        // tag 阶段 4541 / 4647 ms ⟶ **5787 / 5792 ms（+25%）**，而同一实验里 parse / link /
        // resolve 三个阶段逐轮几乎不变（对照组）⇒ 退化可归因于 RETURNING 本身，不是机器变慢。
        // 于是恢复两段式，并加细分计时（upsert / select / file_tag）定位真正的开销。
        // **预编译语句缓存**（rusqlite 的 prepare_cached）：SQL 文本与结果语义**完全不变**，
        // 只是不再每次调用都重新 parse/codegen。依据见细分计时：单条"按唯一索引取 id"的 SELECT
        // 实测 63 µs/条（946 ms ÷ 15000 条），远超已编译语句应有的量级 ⇒ 开销在 prepare 上。
        // prepare_cached 在同一连接上按 SQL 文本复用语句（SQLITE_PREPARE_PERSISTENT），
        // 作用域结束后自动归还（见 rusqlite/src/cache.rs 的 cache_stmt）。
        let t_upsert = start();
        {
            let mut stmt = conn
                .prepare_cached(TAG_UPSERT_SQL)
                .map_err(|e| AppError::db_with("准备标签写入", &e))?;
            stmt.execute(params![
                t.norm,
                t.name,
                depth,
                if t.is_leaf { 1 } else { 0 }
            ])
            .map_err(|e| AppError::db_with("写入标签", &e))?;
        }
        stop(t_upsert, Phase::TagUpsert);
        let t_select = start();
        let tag_id: i64 = {
            let mut stmt = conn
                .prepare_cached(TAG_SELECT_ID_SQL)
                .map_err(|e| AppError::db_with("准备读取标签 id", &e))?;
            stmt.query_row(params![t.norm], |r| r.get(0))
                .map_err(|e| AppError::db_with("读取标签 id", &e))?
        };
        stop(t_select, Phase::TagSelect);
        let t_filetag = start();
        {
            let mut stmt = conn
                .prepare_cached(FILE_TAG_INSERT_SQL)
                .map_err(|e| AppError::db_with("准备文件标签关联", &e))?;
            stmt.execute(params![file_id, tag_id, t.line, t.col as i64])
                .map_err(|e| AppError::db_with("写入文件标签关联", &e))?;
        }
        stop(t_filetag, Phase::TagFileTag);
    }
    stop(t_tags, Phase::Tag);
    let t = start();
    if let Some(fm) = note.frontmatter.as_ref() {
        for alias in &fm.aliases {
            conn.execute(
                "INSERT OR IGNORE INTO file_alias (file_id, alias) VALUES (?1, ?2)",
                params![file_id, alias],
            )
            .map_err(|e| AppError::db_with("写入别名", &e))?;
        }
    }
    stop(t, Phase::Alias);
    Ok(())
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
    mut progress: F,
    mode: IndexMode,
    should_cancel: C,
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
        let indexed = pool.with_writer(move |conn| {
            use crate::index_profile::{start, stop, Phase};
            let t = start();
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| AppError::db_with("开启索引事务", &e))?;
            stop(t, Phase::TxnBegin);
            let mut n = 0usize;
            for entry in &chunk {
                if index_file_mode(conn, &root_owned, entry, mode)? {
                    n += 1;
                }
            }
            let t = start();
            conn.execute_batch("COMMIT")
                .map_err(|e| AppError::db_with("提交索引事务", &e))?;
            stop(t, Phase::TxnCommit);
            Ok(n)
        })?;
        outcome.indexed += indexed;
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
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    Ok(outcome)
}
