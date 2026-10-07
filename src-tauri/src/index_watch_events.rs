//! 文件监听的事件载荷（从 `index_watch` 拆出，避免单文件超 CODE-11 硬上限）。
//!
//! 载荷字段以 **PRD §5.4 为准**（勘误 D-22/D-23）：`created { rel_path, kind }`、
//! `modified { rel_path, mtime_ms }`、`removed { rel_path }`、`renamed { from, to }`。

/// kp://fs/created 载荷。
#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FsCreated {
    pub rel_path: String,
    pub kind: String,
}

/// kp://fs/modified 载荷。
#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FsModified {
    pub rel_path: String,
    pub mtime_ms: i64,
}

/// kp://fs/removed 载荷。
#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FsRemoved {
    pub rel_path: String,
}

/// kp://fs/renamed 载荷。
#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FsRenamed {
    pub from: String,
    pub to: String,
}
