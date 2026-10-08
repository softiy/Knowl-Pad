use crate::storage::pool::DbPool;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 应用状态：当前打开的 Vault 根（M0 仅最小实现，M1 扩展为连接池/队列/取消令牌）。
#[derive(Default)]
pub struct AppState {
    vault_root: Mutex<Option<PathBuf>>,
    /// 全局库连接池（M1 PR-1 接入；打开失败时为 None，应用以默认配置继续，FR-GLOBAL-01）。
    global_db: Mutex<Option<Arc<DbPool>>>,
    /// 当前 Vault 的索引库连接池（M1 PR-2 接入；未打开 Vault 时为 None）。
    index_db: Mutex<Option<Arc<DbPool>>>,
    /// 当前 Vault 在全局库中的注册 id（未注册或全局库不可用时为 None）。
    current_vault_id: Mutex<Option<i64>>,
    /// 索引取消请求（FR-VAULT-09 / AC-VAULT-05）；由 index_cancel 置位、索引循环在批间检查并清除。
    index_cancel: AtomicBool,
    /// 当前 Vault 的文件监听句柄（M3 WP5）；关闭 Vault 或重新打开时替换/停止。
    watcher: Mutex<Option<crate::index_watch::WatcherHandle>>,
    /// 该句柄监听的 Vault 根（M3 复核 blocker 修复）：
    /// 只有带上 root 才能区分「同一个 Vault 重开」与「切换到另一个 Vault」。
    watcher_root: Mutex<Option<PathBuf>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录全局库句柄（启动时调用）。
    pub fn set_global_db(&self, pool: Arc<DbPool>) {
        if let Ok(mut guard) = self.global_db.lock() {
            *guard = Some(pool);
        }
    }

    /// 全局库句柄（尚未就绪时返回 None）。
    pub fn global_db(&self) -> Option<Arc<DbPool>> {
        self.global_db.lock().ok().and_then(|guard| guard.clone())
    }

    /// 记录/清除当前 Vault 的索引库句柄（关闭 Vault 时传 None）。
    pub fn set_index_db(&self, pool: Option<Arc<DbPool>>) {
        if let Ok(mut guard) = self.index_db.lock() {
            *guard = pool;
        }
    }

    /// 当前 Vault 的索引库句柄（未打开 Vault 时返回 None）。
    pub fn index_db(&self) -> Option<Arc<DbPool>> {
        self.index_db.lock().ok().and_then(|guard| guard.clone())
    }

    /// 请求取消当前索引（幂等）。
    pub fn request_index_cancel(&self) {
        self.index_cancel.store(true, Ordering::SeqCst);
    }

    /// 索引循环检查取消请求，并在读取时清除（避免影响下一次索引）。
    pub fn take_index_cancel(&self) -> bool {
        self.index_cancel.swap(false, Ordering::SeqCst)
    }

    /// 记录监听句柄与它监听的 root（替换旧的会先停止它）。
    pub fn set_watcher(&self, handle: crate::index_watch::WatcherHandle, root: PathBuf) {
        if let Ok(mut guard) = self.watcher.lock() {
            if let Some(old) = guard.take() {
                old.stop();
            }
            *guard = Some(handle);
        }
        if let Ok(mut guard) = self.watcher_root.lock() {
            *guard = Some(root);
        }
    }

    /// 当前监听指向的 Vault 根。
    pub fn watcher_root(&self) -> Option<PathBuf> {
        self.watcher_root.lock().ok().and_then(|g| g.clone())
    }

    /// 是否**已经**为这个 root 起了监听。
    ///
    /// M3 复核 blocker：此前只有不带 root 的 `has_watcher()`，打开第二个 Vault 时
    /// 会被误判为"已在监听"，于是新 Vault 永不启动监听、旧 watcher 继续写旧库。
    pub fn has_watcher_for(&self, root: &std::path::Path) -> bool {
        self.watcher_root().map(|r| r == root).unwrap_or(false)
            && self.watcher.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// 停止监听（关闭或**切换** Vault 时调用）。
    pub fn stop_watcher(&self) {
        if let Ok(mut guard) = self.watcher.lock() {
            if let Some(old) = guard.take() {
                old.stop();
            }
        }
        if let Ok(mut guard) = self.watcher_root.lock() {
            *guard = None;
        }
    }

    /// 记录当前 Vault 的注册 id。
    pub fn set_current_vault_id(&self, id: Option<i64>) {
        if let Ok(mut guard) = self.current_vault_id.lock() {
            *guard = id;
        }
    }

    /// 当前 Vault 的注册 id。
    pub fn current_vault_id(&self) -> Option<i64> {
        self.current_vault_id.lock().ok().and_then(|guard| *guard)
    }

    pub fn set_root(&self, root: Option<PathBuf>) {
        if let Ok(mut guard) = self.vault_root.lock() {
            *guard = root;
        }
    }

    /// 当前 Vault 根（未打开时为 None）。
    pub fn current_root(&self) -> Option<PathBuf> {
        self.vault_root.lock().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_pool() -> (tempfile::TempDir, Arc<DbPool>) {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let pool = crate::storage::global::open(dir.path()).expect("打开全局库应成功");
        (dir, Arc::new(pool))
    }

    #[test]
    fn index_db_handle_is_settable_and_clearable() {
        let state = AppState::new();
        assert!(state.index_db().is_none(), "初始应为空");
        let (_dir, pool) = temp_pool();
        state.set_index_db(Some(Arc::clone(&pool)));
        assert!(state.index_db().is_some(), "设置后应可读取");
        state.set_index_db(None);
        assert!(state.index_db().is_none(), "清除后应为空");
    }

    #[test]
    fn global_db_handle_is_settable() {
        let state = AppState::new();
        assert!(state.global_db().is_none());
        let (_dir, pool) = temp_pool();
        state.set_global_db(pool);
        assert!(state.global_db().is_some());
    }
}
