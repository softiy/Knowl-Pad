//! Vault 内部路径约定（FR-STORAGE-01/04）。
//!
//! .knowlpad/ 是软件自有目录：不得出现在文件树、搜索结果、图谱与标签统计中。
//! 本模块只提供**纯函数判定**，供 M2（文件树）与 M3（索引遍历）复用——判定必须唯一，
//! 不允许各调用点自行写字符串比较。

/// Vault 内部目录名（位于 Vault 根下）。
pub const INTERNAL_DIR: &str = ".knowlpad";

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
