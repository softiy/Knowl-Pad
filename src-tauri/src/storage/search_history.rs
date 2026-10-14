//! 搜索历史的**存储**（FR-SEARCH-10：保留最近 20 条查询，「存于全局库」，可一键复用、可清空）。
//!
//! **口径（本 PR 的选择，已在报告里登记待裁定）**：落在 global.db 的 preference 表，
//! 键 search.history，值为 **JSON 字符串数组**（PRD §3.3 明确 preference.value 为 JSON 编码，
//! 因此 preference_get 也能直接把它读成一个数组）。备选口径是 vault_state（每 Vault 一份）——
//! 两者都在 global.db 内，而 FR-SEARCH-10 只写到「存于全局库」这一层；
//! 本实现取与「显示隐藏文件」等设置**当前的实现作用域**（全局 preference）一致的做法，
//! 若所有者裁定应为 Vault 作用域（DEBT-15 的方向），改这里一处即可。
//!
//! 条数与去重语义在 kp-domain::search_history（纯函数、可脱离数据库断言）。

use kp_domain::error::AppError;
use kp_domain::search_history;

use crate::storage::{global, pool::DbPool};

/// global.db 的 preference 键。
pub const HISTORY_KEY: &str = "search.history";

/// 读取历史。**坏数据只降级、不报错**：历史是便利功能，不该把搜索功能卡死；
/// 但降级必须留痕（否则"历史忽然空了"无从排查）。
pub fn load(pool: &DbPool) -> Result<Vec<String>, AppError> {
    let Some(raw) = global::read_preference(pool, HISTORY_KEY)? else {
        return Ok(Vec::new());
    };
    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(list) => Ok(list),
        Err(err) => {
            tracing::warn!(error = %err, "搜索历史格式非法，本次按空历史处理");
            Ok(Vec::new())
        }
    }
}

/// 记入一条查询（去重 + 截断上限），返回写回后的完整历史。
pub fn record(pool: &DbPool, query: &str) -> Result<Vec<String>, AppError> {
    let list = search_history::record(&load(pool)?, query);
    let encoded =
        serde_json::to_string(&list).map_err(|e| AppError::db_with("编码搜索历史", &e))?;
    global::set_preference(pool, HISTORY_KEY, &encoded)?;
    Ok(list)
}

/// 清空历史（幂等：本来就没有记录也算成功）。
pub fn clear(pool: &DbPool) -> Result<(), AppError> {
    global::clear_preference(pool, HISTORY_KEY)
}
