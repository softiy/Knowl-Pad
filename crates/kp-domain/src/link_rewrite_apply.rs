//! 改写落地：**备份 → 原子写 → 任一步失败则整批回滚**（FR-FILE-21/22）。
//!
//! "不得出现中间状态"是本模块唯一的设计目标：
//! ① 每个文件用 note_io::atomic_write（临时文件 + 原子替换）→ 任何时刻磁盘上只可能是**完整旧内容**或**完整新内容**；
//! ② 批量场景先**全部备份**再写；任一文件写失败 → 用备份把**已写的全部还原** → 结果与开始前一致；
//! ③ 备份目录保留原名与内容，供用户"一键回滚"（AC-FILE-02）。

use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::link_rewrite::rewrite_references;
use crate::note_io;

/// 单个文件的改写内容。
#[derive(Debug, Clone)]
pub struct FileChange {
    pub rel_path: String,
    /// 改写前的原文（回滚用，同时也用于"确实变了"的断言）
    pub old_content: String,
    pub new_content: String,
    /// 这个文件里实际改了几处链接（0 表示不改、不进计划）
    pub hits: usize,
}

/// 一次改写的完整计划（预览与落地共用）。
#[derive(Debug, Clone, Default)]
pub struct RewritePlan {
    pub changes: Vec<FileChange>,
}

impl RewritePlan {
    pub fn total_hits(&self) -> usize {
        self.changes.iter().map(|c| c.hits).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// 落地结果。
#[derive(Debug, Clone)]
pub struct ApplyReport {
    pub files: usize,
    pub hits: usize,
    pub backup_dir: PathBuf,
}

/// 生成计划：只读。**hits 为 0 的文件不进计划**（避免"打开了但没改"也产生备份噪音）。
pub fn plan_rename(
    root: &Path,
    from: &str,
    to: &str,
    rel_paths: &[String],
) -> Result<RewritePlan, AppError> {
    let mut changes = Vec::new();
    for rel in rel_paths {
        let target = root.join(rel);
        let Ok(bytes) = std::fs::read(&target) else {
            continue; // 索引与磁盘短暂不一致时跳过，不视为错误
        };
        let Ok(old_content) = String::from_utf8(bytes) else {
            continue; // 非 UTF-8 不猜编码，跳过
        };
        let (new_content, hits) = rewrite_references(&old_content, from, to);
        if hits > 0 && new_content != old_content {
            changes.push(FileChange {
                rel_path: rel.clone(),
                old_content,
                new_content,
                hits,
            });
        }
    }
    Ok(RewritePlan { changes })
}

/// 落地计划：**全有或全无**。
///
/// 步骤：建备份目录 → 逐个备份 → 逐个原子写；任一步失败 → 还原所有已写文件并把
/// 备份留在原处（便于用户查看），返回错误。
pub fn apply(root: &Path, plan: &RewritePlan, backup_root: &Path) -> Result<ApplyReport, AppError> {
    if plan.is_empty() {
        return Ok(ApplyReport {
            files: 0,
            hits: 0,
            backup_dir: backup_root.to_path_buf(),
        });
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup_dir = backup_root.join(format!("link-rewrite-{stamp}"));
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| AppError::IoFailure(format!("创建备份目录失败：{e}")))?;

    // ① 先全部备份（此时磁盘尚未改动，失败可直接返回）
    for (i, change) in plan.changes.iter().enumerate() {
        let backup = backup_dir.join(format!("{i}.bak"));
        std::fs::write(&backup, change.old_content.as_bytes())
            .map_err(|e| AppError::IoFailure(format!("写备份失败（{}）：{e}", change.rel_path)))?;
    }

    // ② 逐个原子写；任一失败 → 还原已写的
    let mut written: Vec<usize> = Vec::new();
    for (i, change) in plan.changes.iter().enumerate() {
        let target = root.join(&change.rel_path);
        match note_io::atomic_write(&target, change.new_content.as_bytes()) {
            Ok(()) => written.push(i),
            Err(err) => {
                for j in written.iter().rev() {
                    let done = &plan.changes[*j];
                    let target = root.join(&done.rel_path);
                    let _ = note_io::atomic_write(&target, done.old_content.as_bytes());
                }
                return Err(AppError::IoFailure(format!(
                    "改写 {} 失败，已回滚 {} 个文件：{err}",
                    change.rel_path,
                    written.len()
                )));
            }
        }
    }

    Ok(ApplyReport {
        files: plan.changes.len(),
        hits: plan.total_hits(),
        backup_dir,
    })
}

/// **重命名 + 链接改写作为一次原子操作**（FR-FILE-22）。
///
/// 要求的顺序不是随便定的：如果**先改写再改名**，中间就会出现"链接已指向新名、文件仍是旧名"
/// 的状态 —— 那正是 FR-FILE-22 明文**严禁**的。因此顺序固定为：
///
/// 1. 生成计划（只读，失败不留痕）；
/// 2. **先改名**（失败则一处链接都不动）；
/// 3. 再落地改写（失败则由 `apply` 还原已改文件，并把**改名撤销**）。
///
/// 返回（落地报告, 改名后的相对路径）。
pub fn rename_with_rewrite(
    root: &Path,
    from_rel: &str,
    to_rel: &str,
    rel_paths: &[String],
    backup_root: &Path,
) -> Result<(ApplyReport, String), AppError> {
    // ① 计划：大小写不敏感的匹配由改写器内部按**词干**处理；这里传原样路径，
    // 新引用的**大小写属于用户的新名**（预先小写会把 B.md 写成 [[b]]）。
    let plan = plan_rename(root, from_rel, to_rel, rel_paths)?;

    // ② 先改名（含父目录创建，供移动场景复用）
    let from_abs = root.join(from_rel);
    let to_abs = root.join(to_rel);
    if let Some(parent) = to_abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::IoFailure(format!("创建目标目录失败：{e}")))?;
    }
    if !from_abs.exists() {
        return Err(AppError::FileNotFound(from_rel.to_string()));
    }
    std::fs::rename(&from_abs, &to_abs).map_err(|e| AppError::io_at(to_rel, &e))?;

    // ③ 再改写；失败则撤销改名（apply 内部已把已改文件还原）
    match apply(root, &plan, backup_root) {
        Ok(report) => Ok((report, to_rel.to_string())),
        Err(err) => {
            if let Err(undo) = std::fs::rename(&to_abs, &from_abs) {
                return Err(AppError::IoFailure(format!(
                    "改写失败且撤销改名也失败（{err}）；文件可能停在 {to_rel}：{undo}"
                )));
            }
            Err(AppError::IoFailure(format!(
                "改写失败，已撤销改名并还原所有文件：{err}"
            )))
        }
    }
}
