//! 文件监听（M3 WP5）：notify + 防抖合并 → 增量索引 + kp://fs/* 事件。
//!
//! 设计要点：
//! - **防抖 200ms**（技术方案 §5.3.3）：保存一次往往触发多个底层事件，合并后再处理；
//! - **忽略规则与遍历同源**（`IGNORED_DIRS` + `.kp-tmp-`），否则监听与索引会各说各话；
//! - 核心逻辑接受**回调**而非 `AppHandle`，因此可以在没有 Tauri 应用的情况下做端到端测试（DEBT-14 的同一思路）；
//! - **轮询回退**：inotify watch 耗尽（NFR-PLAT-09）等场景下退化为每 5s 的「跳过未变更」全量对账。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use tauri::{AppHandle, Emitter};

use kp_domain::error::AppError;

use crate::index_engine::{self, IndexMode, ScanEntry, IGNORED_DIRS};
use crate::storage::pool::DbPool;

use crate::index_watch_events::{FsCreated, FsModified, FsRemoved, FsRenamed};

/// 防抖窗口（技术方案 §5.3.3）。
pub const DEBOUNCE_MS: u64 = 200;
/// 轮询回退周期。
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// 一次防抖窗口内归并出的变更（相对 Vault 根，正斜杠）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Created(String),
    Modified(String),
    Removed(String),
    Renamed(String, String),
}

/// 该路径是否应被忽略（与遍历同源）。
pub fn is_ignored(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return true;
    };
    rel.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name.starts_with(".kp-tmp-") || IGNORED_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d))
    })
}

fn rel_of(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

fn is_markdown(rel: &str) -> bool {
    rel.to_lowercase().ends_with(".md")
}

/// 把 notify 事件归并成变更列表（同路径只保留最后一次，且去掉 create+modify 的重复）。
pub fn merge_events(root: &Path, kinds: Vec<(EventKind, Vec<PathBuf>)>) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (kind, paths) in kinds {
        match kind {
            EventKind::Create(_) => {
                for p in paths {
                    if is_ignored(root, &p) {
                        continue;
                    }
                    if let Some(rel) = rel_of(root, &p) {
                        if seen.insert(format!("c:{rel}")) {
                            out.push(Change::Created(rel));
                        }
                    }
                }
            }
            EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                // 重命名由 debouncer 的 RenameMode 处理；此处按「两个路径」尽力配对
                let mut it = paths.into_iter().filter(|p| !is_ignored(root, p));
                if let (Some(from), Some(to)) = (it.next(), it.next()) {
                    if let (Some(f), Some(t)) = (rel_of(root, &from), rel_of(root, &to)) {
                        if seen.insert(format!("r:{f}->{t}")) {
                            out.push(Change::Renamed(f, t));
                        }
                    }
                }
            }
            EventKind::Modify(_) => {
                for p in paths {
                    if is_ignored(root, &p) {
                        continue;
                    }
                    if let Some(rel) = rel_of(root, &p) {
                        if seen.insert(format!("m:{rel}")) {
                            out.push(Change::Modified(rel));
                        }
                    }
                }
            }
            EventKind::Remove(_) => {
                for p in paths {
                    if is_ignored(root, &p) {
                        continue;
                    }
                    if let Some(rel) = rel_of(root, &p) {
                        if seen.insert(format!("d:{rel}")) {
                            out.push(Change::Removed(rel));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn scan_entry(root: &Path, rel: &str) -> Option<ScanEntry> {
    let abs = root.join(rel);
    let meta = std::fs::metadata(&abs).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some(ScanEntry {
        rel_path: rel.to_string(),
        size_bytes: meta.len(),
        mtime_ms,
    })
}

/// 处理一批变更：增量写库 → 事件回调 → 一次裁决与计数重算。
pub fn apply_changes<F: Fn(&[Change])>(
    pool: &DbPool,
    root: &Path,
    changes: &[Change],
    on_event: F,
) {
    let changes_owned: Vec<Change> = changes.to_vec();
    let root_owned = root.to_path_buf();
    let write = pool.with_writer(move |conn| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| AppError::db("开启增量事务"))?;
        for change in &changes_owned {
            match change {
                Change::Removed(rel) => {
                    conn.execute(
                        "DELETE FROM note_fts WHERE rowid = (SELECT id FROM file WHERE rel_path = ?1)",
                        rusqlite::params![rel],
                    )
                    .map_err(|_| AppError::db("增量删除 FTS"))?;
                    conn.execute("DELETE FROM file WHERE rel_path = ?1", rusqlite::params![rel])
                        .map_err(|_| AppError::db("增量删除文件"))?;
                }
                Change::Renamed(from, to) => {
                    conn.execute(
                        "DELETE FROM note_fts WHERE rowid = (SELECT id FROM file WHERE rel_path = ?1)",
                        rusqlite::params![from],
                    )
                    .map_err(|_| AppError::db("增量删除 FTS"))?;
                    conn.execute("DELETE FROM file WHERE rel_path = ?1", rusqlite::params![from])
                        .map_err(|_| AppError::db("增量删除旧路径"))?;
                    if is_markdown(to) {
                        if let Some(entry) = scan_entry(&root_owned, to) {
                            index_engine::index_file_mode(conn, &root_owned, &entry, IndexMode::Force)?;
                        }
                    }
                }
                Change::Created(rel) | Change::Modified(rel) => {
                    if is_markdown(rel) {
                        if let Some(entry) = scan_entry(&root_owned, rel) {
                            index_engine::index_file_mode(conn, &root_owned, &entry, IndexMode::Force)?;
                        }
                    }
                }
            }
        }
        conn.execute_batch("COMMIT")
            .map_err(|_| AppError::db("提交增量事务"))?;
        Ok(())
    });
    if let Err(err) = write {
        tracing::warn!(error = %err, "增量索引失败");
    }
    on_event(changes);
    // 单篇变更也会影响链接裁决与标签计数；这里统一重算（十万文件规模实测约 2.4s，可接受）
    let _ = pool.with_writer(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| AppError::db("开启裁决事务"))?;
        crate::index_resolve::resolve_links(conn)?;
        crate::index_resolve::recount_tags(conn)?;
        conn.execute_batch("COMMIT")
            .map_err(|_| AppError::db("提交裁决事务"))?;
        Ok(())
    });
}

/// 监听句柄：持有 debouncer；调用 [`WatcherHandle::stop`] 或 drop 即停止。
pub struct WatcherHandle {
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl WatcherHandle {
    pub fn stop(mut self) {
        if let Some(d) = self.debouncer.take() {
            d.stop();
        }
    }
}

/// 启动监听（核心）：变更经归并后交给回调（**不依赖 Tauri**，便于测试）。
pub fn start_watcher_with<F>(
    pool: Arc<DbPool>,
    root: PathBuf,
    on_changes: F,
) -> Result<WatcherHandle, AppError>
where
    F: Fn(&[Change]) + Send + 'static,
{
    let root_for_handler = root.clone();
    let pool_for_handler = pool.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(DEBOUNCE_MS),
        None,
        move |res: DebounceEventResult| match res {
            Ok(events) => {
                let kinds: Vec<(EventKind, Vec<PathBuf>)> = events
                    .into_iter()
                    .map(|e| (e.event.kind, e.event.paths.clone()))
                    .collect();
                let changes = merge_events(&root_for_handler, kinds);
                if !changes.is_empty() {
                    apply_changes(&pool_for_handler, &root_for_handler, &changes, &on_changes);
                }
            }
            Err(errors) => {
                for e in errors {
                    tracing::warn!(error = %e, "文件监听错误");
                }
            }
        },
    )
    .map_err(|e| AppError::from(std::io::Error::other(format!("启动文件监听失败：{e}"))))?;
    debouncer
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| AppError::from(std::io::Error::other(format!("监听 Vault 失败：{e}"))))?;
    Ok(WatcherHandle {
        debouncer: Some(debouncer),
    })
}

/// 启动监听（带 Tauri 事件）：把变更翻译成 kp://fs/* 事件。
pub fn start_watcher(
    pool: Arc<DbPool>,
    root: PathBuf,
    app: AppHandle,
) -> Result<WatcherHandle, AppError> {
    start_watcher_with(pool, root, move |changes| {
        for change in changes {
            match change {
                Change::Created(rel) => {
                    let _ = app.emit(
                        "kp://fs/created",
                        FsCreated {
                            rel_path: rel.clone(),
                            kind: if is_markdown(rel) {
                                "note".to_string()
                            } else {
                                "file".to_string()
                            },
                        },
                    );
                }
                Change::Modified(rel) => {
                    let _ = app.emit(
                        "kp://fs/modified",
                        FsModified {
                            rel_path: rel.clone(),
                            mtime_ms: 0,
                        },
                    );
                }
                Change::Removed(rel) => {
                    let _ = app.emit(
                        "kp://fs/removed",
                        FsRemoved {
                            rel_path: rel.clone(),
                        },
                    );
                }
                Change::Renamed(from, to) => {
                    let _ = app.emit(
                        "kp://fs/renamed",
                        FsRenamed {
                            from: from.clone(),
                            to: to.clone(),
                        },
                    );
                }
            }
        }
    })
}

/// 轮询回退（inotify 耗尽等，NFR-PLAT-09）：每 5s 做一次「跳过未变更」的对账。
pub fn start_polling_fallback(pool: Arc<DbPool>, root: PathBuf) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        match index_engine::full_index_cancellable(
            &pool,
            &root,
            |_, _| {},
            IndexMode::SkipUnchanged,
            || false,
        ) {
            Ok(outcome) => {
                if outcome.indexed > 0 {
                    tracing::info!(indexed = outcome.indexed, "轮询对账已索引");
                }
            }
            Err(err) => tracing::warn!(error = %err, "轮询对账失败"),
        }
    })
}
