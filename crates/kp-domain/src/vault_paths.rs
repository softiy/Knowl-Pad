//! Vault 内部路径约定（FR-STORAGE-01/04）。
//!
//! .knowlpad/ 是软件自有目录：不得出现在文件树、搜索结果、图谱与标签统计中。
//! 本模块只提供**纯函数判定**，供 M2（文件树）与 M3（索引遍历）复用——判定必须唯一，
//! 不允许各调用点自行写字符串比较。

/// **零接触目录**：FR-STORAGE-02 要求「不得修改、移动或删除其中任何内容」，
/// 因此任何遍历/清理都必须**整目录跳过**（连我们自己的临时文件也不在其中清理——
/// 那同样属于"删除其中内容"）。与 §5.4 的 IGNORED_DIRS 概念一致。
pub const NEVER_TOUCH_DIRS: &[&str] = &[".obsidian", ".git"];

/// Vault 内部目录名（位于 Vault 根下）。
pub const INTERNAL_DIR: &str = ".knowlpad";
/// 覆盖前备份目录（AC-FILE-04、PRD §6.2.1 步骤 4；保留策略见 NFR-REL-05）。
pub const BACKUP_DIR: &str = "backup";
/// 回收站目录（FR-TRASH-02：`.knowlpad/trash/<yyyy-MM>/`）。
pub const TRASH_DIR: &str = "trash";
/// 回收站清单（FR-TRASH-02/12：原始相对路径的权威记录）。
pub const TRASH_MANIFEST: &str = "manifest.json";

/// 判断 Vault 相对路径是否属于内部目录（.knowlpad/**）。
///
/// 只判定**根级**内部目录：嵌套的 .knowlpad（如 notes/.knowlpad/x）是用户的普通目录，
/// 不应被隐藏。比较不区分大小写——Windows/macOS 默认大小写不敏感，
/// 大小写变体指向同一目录，必须一并排除。
pub fn is_internal_path(rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/");
    match normalized.split('/').next() {
        Some(first) => first.eq_ignore_ascii_case(INTERNAL_DIR),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_dir_and_children_are_excluded() {
        for path in [
            ".knowlpad",
            ".knowlpad/index.db",
            ".knowlpad/trash/a.md",
            ".KNOWLPAD/index.db",
            ".knowlpad\\index.db",
        ] {
            assert!(is_internal_path(path), "{path} 应被排除");
        }
    }

    #[test]
    fn third_party_dirs_are_never_touched() {
        // FR-STORAGE-02：第三方配置目录必须"零接触"
        assert!(NEVER_TOUCH_DIRS.contains(&".obsidian"));
        assert!(NEVER_TOUCH_DIRS.contains(&".git"));
        assert!(
            !NEVER_TOUCH_DIRS.contains(&INTERNAL_DIR),
            ".knowlpad 由软件自管，不在零接触清单"
        );
    }

    #[test]
    fn user_files_are_not_excluded() {
        for path in [
            "note.md",
            "notes/.knowlpad/x.md",
            "notes/a.md",
            ".knowlpadx/a.md",
            "a/.knowlpad",
            "",
        ] {
            assert!(!is_internal_path(path), "{path} 不应被排除");
        }
    }
}
