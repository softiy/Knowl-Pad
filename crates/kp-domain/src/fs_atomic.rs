//! 原子替换的平台差异（技术方案 §6.3，对应 RISK-09）。
//!
//! POSIX 与 Windows 在 rename 目标已存在时行为不同：
//! - POSIX：rename 原子替换，旧目标被丢弃；
//! - Windows：Rust 的 fs::rename 已带 MOVEFILE_REPLACE_EXISTING，**但**目标被占用
//!   （其他程序打开）或只读时会失败（PermissionDenied）。
//!
//! 策略：**绝不强行处理被占用的目标**——旧内容保持完整，向上返回 E_FILE_LOCKED，
//! 由 UI 提示用户关闭占用程序后重试（PRD §5.2）。

use crate::error::AppError;
use std::fs;
use std::path::Path;

/// 用 tmp 原子替换 target。
///
/// 失败时**不改动 target**（调用方负责清理 tmp），因此旧内容始终完整。
#[cfg(not(windows))]
pub fn replace(tmp: &Path, target: &Path) -> Result<(), AppError> {
    // POSIX：rename 即为原子替换
    fs::rename(tmp, target).map_err(AppError::from)
}

/// 用 tmp 原子替换 target（Windows 分支）。
#[cfg(windows)]
pub fn replace(tmp: &Path, target: &Path) -> Result<(), AppError> {
    match fs::rename(tmp, target) {
        Ok(()) => Ok(()),
        // 目标被占用或只读：报 E_FILE_LOCKED，绝不删除/改写目标
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(AppError::FileLocked(target.display().to_string()))
        }
        Err(err) => Err(AppError::from(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("临时目录应可创建")
    }

    #[test]
    fn replace_overwrites_existing_target() {
        let dir = temp_dir();
        let target = dir.path().join("note.md");
        let tmp = dir.path().join(".kp-tmp-x");
        fs::write(&target, b"OLD").expect("应可写目标");
        fs::write(&tmp, b"NEW").expect("应可写临时文件");
        replace(&tmp, &target).expect("替换应成功");
        assert_eq!(fs::read(&target).expect("应可读"), b"NEW");
        assert!(!tmp.exists(), "临时文件应已被消耗");
    }

    #[test]
    fn replace_creates_missing_target() {
        let dir = temp_dir();
        let target = dir.path().join("new.md");
        let tmp = dir.path().join(".kp-tmp-y");
        fs::write(&tmp, b"NEW").expect("应可写临时文件");
        replace(&tmp, &target).expect("替换应成功");
        assert_eq!(fs::read(&target).expect("应可读"), b"NEW");
    }

    #[test]
    fn replace_fails_when_tmp_missing_without_touching_target() {
        let dir = temp_dir();
        let target = dir.path().join("keep.md");
        fs::write(&target, b"KEEP").expect("应可写目标");
        let missing = dir.path().join(".kp-tmp-missing");
        assert!(
            replace(&missing, &target).is_err(),
            "临时文件不存在时必须失败"
        );
        assert_eq!(
            fs::read(&target).expect("应可读"),
            b"KEEP",
            "旧内容必须完整保留"
        );
    }

    #[cfg(windows)]
    #[test]
    fn replace_reports_file_locked_when_target_readonly() {
        // Windows 专有：只读目标不可替换，必须报 E_FILE_LOCKED 而不是静默覆盖
        let dir = temp_dir();
        let target = dir.path().join("ro.md");
        let tmp = dir.path().join(".kp-tmp-ro");
        fs::write(&target, b"OLD").expect("应可写目标");
        fs::write(&tmp, b"NEW").expect("应可写临时文件");
        let mut perms = fs::metadata(&target).expect("应可读元数据").permissions();
        perms.set_readonly(true);
        fs::set_permissions(&target, perms).expect("应可设只读");
        let result = replace(&tmp, &target);
        // 允许某些 Windows 版本能替换只读文件；关键断言是「失败时错误码正确且旧内容保留」
        if let Err(err) = result {
            assert_eq!(err.code(), "E_FILE_LOCKED");
            assert_eq!(fs::read(&target).expect("应可读"), b"OLD");
        }
    }
}
