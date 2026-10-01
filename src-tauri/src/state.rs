use crate::error_wrapper::KpError;
use crate::storage::pool::DbPool;
use kp_domain::path_guard::PathGuard;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 应用状态：当前打开的 Vault 根（M0 仅最小实现，M1 扩展为连接池/队列/取消令牌）。
#[derive(Default)]
pub struct AppState {
    vault_root: Mutex<Option<PathBuf>>,
    /// 全局库连接池（M1 PR-1 接入；打开失败时为 None，应用以默认配置继续，FR-GLOBAL-01）。
    global_db: Mutex<Option<Arc<DbPool>>>,
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

    pub fn set_root(&self, root: Option<PathBuf>) {
        if let Ok(mut guard) = self.vault_root.lock() {
            *guard = root;
        }
    }

    /// 当前 Vault 根（未打开时为 None）。
    pub fn current_root(&self) -> Option<PathBuf> {
        self.vault_root.lock().ok().and_then(|g| g.clone())
    }

    /// 取路径校验器；未打开 Vault 时返回 E_VAULT_NOT_OPEN。
    pub fn guard(&self) -> Result<PathGuard, KpError> {
        let root = self.vault_root.lock().map_err(|_| {
            KpError(kp_domain::error::AppError::IoFailure(
                "state lock poisoned".into(),
            ))
        })?;
        match root.as_ref() {
            Some(path) => PathGuard::new(path).map_err(KpError),
            None => Err(KpError(kp_domain::error::AppError::VaultNotOpen)),
        }
    }
}
