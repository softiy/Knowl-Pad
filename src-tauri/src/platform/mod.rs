//! 平台差异适配（NFR-PLAT-15）。M0 只提供最小信息，M1 起承载 rename 语义、权限与菜单。

/// 当前平台是否为大小写不敏感的文件系统（Windows/macOS 默认是）。
pub fn case_insensitive_fs() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}
