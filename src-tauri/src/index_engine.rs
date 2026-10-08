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
use walkdir::WalkDir;

use kp_domain::error::AppError;
use kp_domain::md_parse;
use kp_domain::tokenize;

use crate::storage::pool::DbPool;

/// 遍历时永远跳过的目录（与监听侧共用；文件树也复用它保持同一套真相）。
pub const IGNORED_DIRS: [&str; 4] = [".knowlpad", ".obsidian", ".git", "node_modules"];
/// 超过此大小的笔记跳过（SEC-11）。
pub const MAX_NOTE_BYTES: u64 = 5 * 1024 * 1024;
/// 每批提交的文件数（PERF-05：禁止逐条事务）。
pub const BATCH_SIZE: usize = 200;

/// 待索引的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    pub rel_path: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
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

fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn scan_vault(root: &Path) -> Result<(Vec<ScanEntry>, Vec<String>), AppError> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if name.starts_with(".kp-tmp-") {
            return false;
        }
        !(e.file_type().is_dir() && IGNORED_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d)))
    });
    for item in walker {
        let entry = match item {
            Ok(e) => e,
            Err(err) => {
                warnings.push(format!("遍历告警:{err}"));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // PR-6：记录**全部**文件（附件也要出现在文件树里）；只有 .md 会被解析并建全文索引
        let is_note = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"));
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let rel_path = path
            .strip_prefix(root)
            .map_err(|_| AppError::PathOutsideVault)?
            .to_string_lossy()
            .replace('\\', "/");
        if is_note && size_bytes > MAX_NOTE_BYTES {
            warnings.push(format!("skip-oversize:{rel_path}"));
            continue;
        }
        entries.push(ScanEntry {
            rel_path,
            size_bytes,
            mtime_ms: mtime_ms(path),
        });
    }
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok((entries, warnings))
}

/// 该目录名是否在索引忽略集里（文件树复用，避免两套真相）。
pub fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
}

/// 简易内容指纹（FNV-1a 64）：仅供增量索引判断「内容是否变化」，不承担安全用途。
pub fn content_fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn kind_of(rel_path: &str) -> &'static str {
    let ext = rel_path.rsplit('.').next().unwrap_or("").to_lowercase();
    if ext == "md" {
        return "note";
    }
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "pdf" | "mp3" | "wav" | "m4a"
        | "mp4" | "mov" | "zip" | "txt" | "csv" | "json" => "attachment",
        _ => "other",
    }
}

fn split_name(rel_path: &str) -> (String, String, String) {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path).to_string();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => (name.clone(), String::new()),
    };
    (name, stem, ext)
}

/// 跳过策略：Force 一律重建；SkipUnchanged 时 mtime 与大小都未变的文件直接跳过。
///
/// 说明：mtime + size 相同但内容被改的情况检测不到（完整指纹比对待 PR-5 的增量索引补），
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
    let abs = root.join(&entry.rel_path);
    let kind = kind_of(&entry.rel_path);
    let is_note = kind == "note";
    let bytes = std::fs::read(&abs).map_err(AppError::from)?;
    let note = if is_note {
        Some(md_parse::parse(&bytes))
    } else {
        None
    };
    let indexed_text = note
        .as_ref()
        .map(|n| tokenize::for_index(&n.plain_text))
        .unwrap_or_default();
    let fingerprint = content_fingerprint(&bytes);
    let now = mtime_ms(&abs).max(1);

    // 旧 rowid（note_fts 的 rowid 与 file.id 对齐）必须在删 file 之前取
    let old_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM file WHERE rel_path = ?1",
            params![entry.rel_path],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = old_id {
        conn.execute("DELETE FROM note_fts WHERE rowid = ?1", params![id])
            .map_err(|_| AppError::db("清理旧 FTS 行"))?;
        conn.execute("DELETE FROM file WHERE id = ?1", params![id])
            .map_err(|_| AppError::db("清理旧文件行"))?;
    }

    let (name, stem, ext) = split_name(&entry.rel_path);
    conn.execute(
        "INSERT INTO file (rel_path, name, stem, ext, kind, size_bytes, mtime_ms, content_hash, deleted, indexed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
        params![entry.rel_path, name, stem, ext, kind, entry.size_bytes as i64, entry.mtime_ms, fingerprint, now],
    )
    .map_err(|_| AppError::db("写入文件行"))?;
    let file_id = conn.last_insert_rowid();

    if is_note {
        conn.execute(
            "INSERT INTO note_fts(rowid, plain_text) VALUES (?1, ?2)",
            params![file_id, indexed_text],
        )
        .map_err(|_| AppError::db("写入全文索引"))?;
    }

    let Some(note) = note.as_ref() else {
        return Ok(true);
    };
    for (i, h) in note.headings.iter().enumerate() {
        conn.execute(
            "INSERT INTO heading (file_id, level, text, anchor, line, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![file_id, h.level as i64, h.text, h.text, h.line as i64, i as i64],
        )
        .map_err(|_| AppError::db("写入标题"))?;
    }
    for b in &note.block_ids {
        conn.execute(
            "INSERT OR IGNORE INTO block_id (file_id, bid, line_start, line_end) VALUES (?1, ?2, ?3, ?4)",
            params![file_id, b.id, b.start_line as i64, b.end_line as i64],
        )
        .map_err(|_| AppError::db("写入块 ID"))?;
    }
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
        .map_err(|_| AppError::db("写入链接"))?;
    }
    for t in &note.tags {
        let depth = t.name.split('/').count() as i64;
        conn.execute(
            "INSERT INTO tag (norm, display, depth, is_leaf, ref_count) VALUES (?1, ?2, ?3, ?4, 0) \
             ON CONFLICT(norm) DO UPDATE SET display = excluded.display, depth = excluded.depth, is_leaf = excluded.is_leaf",
            params![t.norm, t.name, depth, if t.is_leaf { 1 } else { 0 }],
        )
        .map_err(|_| AppError::db("写入标签"))?;
        let tag_id: i64 = conn
            .query_row("SELECT id FROM tag WHERE norm = ?1", params![t.norm], |r| {
                r.get(0)
            })
            .map_err(|_| AppError::db("读取标签 id"))?;
        conn.execute(
            "INSERT INTO file_tag (file_id, tag_id, line, col) VALUES (?1, ?2, ?3, ?4)",
            params![file_id, tag_id, t.line, t.col as i64],
        )
        .map_err(|_| AppError::db("写入文件标签关联"))?;
    }
    if let Some(fm) = note.frontmatter.as_ref() {
        for alias in &fm.aliases {
            conn.execute(
                "INSERT OR IGNORE INTO file_alias (file_id, alias) VALUES (?1, ?2)",
                params![file_id, alias],
            )
            .map_err(|_| AppError::db("写入别名"))?;
        }
    }
    Ok(true)
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
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|_| AppError::db("开启索引事务"))?;
            let mut n = 0usize;
            for entry in &chunk {
                if index_file_mode(conn, &root_owned, entry, mode)? {
                    n += 1;
                }
            }
            conn.execute_batch("COMMIT")
                .map_err(|_| AppError::db("提交索引事务"))?;
            Ok(n)
        })?;
        outcome.indexed += indexed;
        done += chunk_len;
        progress(done, total);
    }
    // 全部文件写完后统一裁决（此时候选行才齐）；裁决与计数在同一事务内完成
    pool.with_writer(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| AppError::db("开启裁决事务"))?;
        crate::index_resolve::resolve_links(conn)?;
        crate::index_resolve::recount_tags(conn)?;
        conn.execute_batch("COMMIT")
            .map_err(|_| AppError::db("提交裁决事务"))?;
        Ok(())
    })?;
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    Ok(outcome)
}
