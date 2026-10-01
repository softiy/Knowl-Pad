use crate::storage::pool::DbPool;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 应用状态：当前打开的 Vault 根（M0 仅最小实现，M1 扩展为连接池/队列/取消令牌）。
#[derive(Default)]
pub struct AppState {
    vault_root: Mutex<Option<PathBuf>>,
    /// 全局库连接池（M1 PR-1 接入；打开失败时为 None，应用以默认配置继续，FR-GLOBAL-01）。
    global_db: Mutex<Option<Arc<DbPool>>>,
    /// 当前 Vault 的索引库连接池（M1 PR-2 接入；未打开 Vault 时为 None）。
    index_db: Mutex<Option<Arc<DbPool>>>,
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
