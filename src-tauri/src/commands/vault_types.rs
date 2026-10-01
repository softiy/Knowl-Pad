//! Vault / 索引的 IPC 数据结构（CODE-11：拆出以维持单文件行数上限）。

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultOpenArgs {
    pub abs_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultCreateArgs {
    pub abs_path: String,
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultIdArgs {
    pub vault_id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRenameArgs {
    pub vault_id: i64,
    pub display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRelocateArgs {
    pub vault_id: i64,
    pub new_abs_path: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub root: String,
    pub display_name: String,
    /// 全局库中的注册 id（全局库不可用时为 None）
    pub vault_id: Option<i64>,
    pub case_insensitive_fs: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened: Option<i64>,
    pub pinned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub index_dir: String,
    pub index_db: String,
    pub ready: bool,
    /// 索引库 schema 版本（未就绪时为 None）
    pub schema_version: Option<i64>,
    /// 索引签名摘要前 12 位（用于 UI 诊断；未就绪时为 None）
    pub signature_prefix: Option<String>,
    /// 上次建库时间戳（毫秒）
    pub built_at: Option<i64>,
}
