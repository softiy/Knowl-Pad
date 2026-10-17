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
) -> Result<(), AppError> {
    let changes_owned: Vec<Change> = changes.to_vec();
    let root_owned = root.to_path_buf();
    let write = pool.with_writer(move |conn| {
        // 用 Transaction 而不是手工 execute_batch("BEGIN")：闭包里任何 `?` 提前返回时
        // 它会 **drop 即 ROLLBACK**。此前是手工 BEGIN + 结尾 COMMIT，中途出错没有 ROLLBACK，
        // 写连接会永久卡在打开的事务上（之后每次 BEGIN 都报
        // "cannot start a transaction within a transaction"），增量索引从此静默失效。
        // 复现见 index_watch_tests::failed_batch_does_not_leave_the_connection_stuck_in_a_transaction。
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::db_with("开启增量事务", &e))?;
        let conn = &tx;
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
                    .map_err(|e| AppError::db_with("增量删除 FTS", &e))?;
                    conn.execute("DELETE FROM file WHERE rel_path = ?1", rusqlite::params![rel])
                        .map_err(|e| AppError::db_with("增量删除文件", &e))?;
                }
                Change::Renamed(from, to) => {
                    conn.execute(
                        "DELETE FROM note_fts WHERE rowid = (SELECT id FROM file WHERE rel_path = ?1)",
                        rusqlite::params![from],
                    )
                    .map_err(|e| AppError::db_with("增量删除 FTS", &e))?;
                    conn.execute("DELETE FROM file WHERE rel_path = ?1", rusqlite::params![from])
                        .map_err(|e| AppError::db_with("增量删除旧路径", &e))?;
                    if is_markdown(to) {
                        if let Some(entry) = scan_entry(&root_owned, to) {
                            // 增量路径只关心"写没写成"：超时（FileOutcome::TimedOut）已在校验侧记日志，
                            // 这里不额外处理 —— 该文件的索引行保持上一次的状态，等下次变更/重建再收。
                            let _ = index_engine::index_file_mode(
                                conn,
                                &root_owned,
                                &entry,
                                IndexMode::Force,
                            )?;
                        }
                    }
                }
                Change::Created(rel) | Change::Modified(rel) => {
                    if is_markdown(rel) {
                        if let Some(entry) = scan_entry(&root_owned, rel) {
                            let _ = index_engine::index_file_mode(
                                conn,
                                &root_owned,
                                &entry,
                                IndexMode::Force,
                            )?;
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
        // ⑤ 收尾：清理**孤儿标签行**（PRD 勘误 **D-42 候选 (a)** 的落地）。
        //
        // **为什么在这里删是安全的，而在全量重建中途不安全**：
        // · 增量是**单文件级**的 —— 这一批文件已经全部写完并 `recount_tags` 过，此刻 `ref_count = 0`
        //   且没有 `file_tag` 引用的标签，**确实**没有任何笔记再用它（不是"还没轮到"），删掉不会误伤；
        // · 全量重建中途某标签可能**暂时**为 0（它的文件还没轮到），早删会把后面还要用的标签删掉 ——
        //   所以那边只在收尾（全部文件写完之后、未取消时）删，见 `index_engine.rs` 的同一行调用。
        //
        // 触发时机：每次增量事务**提交前**，与写入 / 裁决 / 计数在**同一事务**里 —— 要么全成、要么全不成。
        conn.execute(crate::index_sql::ORPHAN_TAGS_DELETE_SQL, [])
            .map_err(|e| AppError::db_with("清理孤儿标签", &e))?;
        tx.commit()
            .map_err(|e| AppError::db_with("提交增量事务", &e))?;
        Ok(())
    });
    match write {
        Ok(()) => {
            // **只有写库成功才告诉前端"文件已更新"**：此前无论成败都发事件，
            // 于是 UI 按"事件到了"去刷新，读到的却还是旧索引（搜索/反链/悬空全旧），
            // 而徽标仍显示"就绪" —— 失败对用户与 CI 都不可见。
            on_event(changes);
            Ok(())
        }
        Err(err) => {
            tracing::error!(error = %err, "增量索引失败：本次不上报文件事件（避免 UI 读到旧索引）");
            Err(err)
        }
    }
    // 单篇变更也会影响链接裁决与标签计数；这里统一重算（十万文件规模实测约 2.4s，可接受）
}

/// 监听句柄：持有 debouncer；调用 [`WatcherHandle::stop`] 或 drop 即停止。
pub struct WatcherHandle {
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
    /// 轮询回退线程的**停止标志**（审计 T-21）。
    ///
    /// 此前 `start_polling_fallback` 的循环用 `|| false` 当取消谓词、句柄又被 `let _ =` 丢掉：
    /// 一旦降级就**永远停不下来** —— 关掉 Vault/退出前每 5 秒仍在做全量对账，白抢磁盘 IO。
    fallback_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl WatcherHandle {
    pub fn stop(mut self) {
        // 先让轮询回退线程退出（审计 T-21），再停 debouncer。
        self.fallback_stop
            .store(true, std::sync::atomic::Ordering::SeqCst);
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
    // 轮询回退的停止标志（审计 T-21）：句柄持有一份，事件处理器持有一份。
    let fallback_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fallback_stop_for_handle = fallback_stop.clone();
    let fallback_stop_for_handler = fallback_stop.clone();
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
                    if let Err(err) = apply_changes(
                        &pool_for_handler,
                        &root_for_handler,
                        &changes,
                        &on_changes,
                    ) {
                        // 写库失败要**说出来**：让上层发 kp://index/failed（前端据此把徽标置为异常），
                        // 而不是只留一行日志、界面继续显示"就绪"。
                        on_error("E_INDEX_WRITE", &format!("增量索引写入失败：{err}"));
                    }
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
                                fallback_stop_for_handler.clone(),
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
        fallback_stop: fallback_stop_for_handle,
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

pub use crate::index_watch_poll::start_polling_fallback;
