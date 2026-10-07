//! 文件监听（M3 WP5）：notify + 防抖合并 → 增量索引 + kp://fs/* 事件。
//!
//! 设计要点：
//! - **防抖 200ms**（技术方案 §5.3.3）：保存一次往往触发多个底层事件，合并后再处理；
//! - **忽略规则与遍历同源**（`IGNORED_DIRS` + `.kp-tmp-`），否则监听与索引会各说各话；
//! - 核心逻辑接受**回调**而非 `AppHandle`，因此可以在没有 Tauri 应用的情况下做端到端测试（DEBT-14 的同一思路）；
//! - **轮询回退**：inotify watch 耗尽（NFR-PLAT-09）等场景下退化为每 5s 的「跳过未变更」全量对账。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use tauri::{AppHandle, Emitter};

use kp_domain::error::AppError;

use crate::index_engine::{self, IndexMode};
use crate::index_watch_paths::{collect_keys, merge_events, rels_of, scan_entry};
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

/// 读取某文件真实 mtime（毫秒）；读不到返回 0。供 kp://fs/modified 载荷使用（PRD §5.4：
/// FR-EDITOR-34 的冲突检测依赖它，独立审查指出此前恒为 0）。
fn mtime_of(root: &Path, rel: &str) -> i64 {
    std::fs::metadata(root.join(rel))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn is_markdown(rel: &str) -> bool {
    rel.to_lowercase().ends_with(".md")
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
        // ① 写库前采集目标键（被删/改名的旧行也在此时还能读到别名）
        let mut target_keys: Vec<String> = Vec::new();
        for change in &changes_owned {
            for rel in rels_of(change) {
                collect_keys(conn, &rel, &mut target_keys)?;
            }
        }
        // ② 逐条写库
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
        // ③ 变更文件的 id（写完后查，新建的也在）
        let mut changed_ids: Vec<i64> = Vec::new();
        for change in &changes_owned {
            for rel in rels_of(change) {
                if let Ok(id) = conn.query_row(
                    "SELECT id FROM file WHERE rel_path = ?1",
                    rusqlite::params![rel],
                    |r| r.get::<_, i64>(0),
                ) {
                    changed_ids.push(id);
                }
            }
        }
        // ④ 只重算受影响的链接（增量裁决）+ 标签计数，与写库同一事务
        crate::index_resolve::resolve_links_matching(conn, &changed_ids, &target_keys)?;
        crate::index_resolve::recount_tags(conn)?;
        conn.execute_batch("COMMIT")
            .map_err(|_| AppError::db("提交增量事务"))?;
        Ok(())
    });
    if let Err(err) = write {
        tracing::warn!(error = %err, "增量索引失败");
    }
    on_event(changes);
    // 单篇变更也会影响链接裁决与标签计数；这里统一重算（十万文件规模实测约 2.4s，可接受）
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
pub fn start_watcher_with<F, G>(
    pool: Arc<DbPool>,
    root: PathBuf,
    on_changes: F,
    on_error: G,
) -> Result<WatcherHandle, AppError>
where
    F: Fn(&[Change]) + Send + 'static,
    G: Fn(&str, &str) + Send + 'static,
{
    let root_for_handler = root.clone();
    let pool_for_handler = pool.clone();
    // 轮询降级只允许启动一次（MaxFilesWatch 可能反复上报）。
    let fallback_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fallback_flag = fallback_started.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(DEBOUNCE_MS),
        None,
        move |res: DebounceEventResult| match res {
            Ok(events) => {
                // Rescan（Windows 事件缓冲区溢出等，TECH:3086）：退化为全量 mtime 对账
                if events.iter().any(|e| e.event.need_rescan()) {
                    tracing::warn!("收到 Rescan，触发全量 mtime 对账");
                    let _ = index_engine::full_index_cancellable(
                        &pool_for_handler,
                        &root_for_handler,
                        |_, _| {},
                        IndexMode::SkipUnchanged,
                        || false,
                    );
                    return;
                }
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
                    // NFR-PLAT-09 / 勘误 D-23：sysctl 建议并入 message（载荷不得新增 detail 字段）
                    if matches!(e.kind, notify::ErrorKind::MaxFilesWatch) {
                        // 独立审查指出：此前只发事件、**并没有真的降级**。这里真正启动轮询对账（只一次）。
                        if !fallback_flag.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            tracing::warn!("监听句柄耗尽，启动 5 秒轮询对账降级");
                            let _ = crate::index_watch::start_polling_fallback(
                                pool_for_handler.clone(),
                                root_for_handler.clone(),
                            );
                        }
                        tracing::warn!(error = %e, "文件监听句柄数达上限，退化为轮询对账");
                        on_error(
                            "E_WATCH_LIMIT",
                            "文件监听句柄数已达系统上限（Linux 可调大 fs.inotify.max_user_watches 后重启），已退化为每 5 秒轮询对账",
                        );
                    } else {
                        tracing::warn!(error = %e, "文件监听错误");
                    }
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

/// 启动监听（带 Tauri 事件）：把变更翻译成 kp://fs/* 事件，监听不可用时报失败事件。
pub fn start_watcher(
    pool: Arc<DbPool>,
    root: PathBuf,
    app: AppHandle,
) -> Result<WatcherHandle, AppError> {
    let error_app = app.clone();
    let root_for_events = root.clone();
    start_watcher_with(
        pool,
        root,
        move |changes| {
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
                                mtime_ms: mtime_of(&root_for_events, rel),
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
        },
        move |_code, message| {
            // 监听不可用（inotify 耗尽等）：按 PRD §5.4 发失败事件（无 detail 字段，勘误 D-23）
            let _ = error_app.emit(
                "kp://index/failed",
                crate::commands::index::IndexFailed {
                    code: "E_WATCH_LIMIT".to_string(),
                    message: message.to_string(),
                    failed_files: Vec::new(),
                },
            );
        },
    )
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
