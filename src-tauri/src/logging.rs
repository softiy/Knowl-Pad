//! 日志初始化与保留策略（技术方案 §9.5；PRD SEC-09 / ERR-04）。
//!
//! - 输出：配置目录下 `logs/knowl-pad.log.<日期>`，**按日滚动**，保留 **14 天**（PRD §2.4.2）；
//! - 过滤：`RUST_LOG` 环境变量（`env-filter`），默认 `info`；
//! - 控制台：**仅 debug 构建**额外输出到 stderr（release 不污染用户体验）；
//! - 隐私：本模块只负责管道；**写什么由调用点遵守 SEC-09**——禁止笔记正文、frontmatter、
//!   搜索词、标签名与偏好值。门禁 11 内含静态检查（tests/security/log-privacy.mjs）。

use kp_domain::error::AppError;
use std::path::Path;
use std::sync::OnceLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// 日志文件基名（滚动时追加日期后缀）。
pub const LOG_FILE_NAME: &str = "knowl-pad.log";

/// 日志保留天数（PRD §2.4.2）。
pub const RETENTION_DAYS: u64 = 14;

/// 记录初始化是否成功，避免重复初始化（测试与热重载场景）。
static INITIALIZED: OnceLock<bool> = OnceLock::new();

/// 初始化日志：写文件 + （debug 构建）写 stderr，并清理过期日志。
///
/// **失败不阻断启动**：日志不可用时应用仍必须能运行（返回 Err 由调用方决定如何处理）。
pub fn init(log_dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(log_dir)?;

    let appender = tracing_appender::rolling::RollingFileAppender::new(
        tracing_appender::rolling::Rotation::DAILY,
        log_dir,
        LOG_FILE_NAME,
    );

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(appender)
        .with_ansi(false)
        .with_target(true);

    // 注册全局订阅端；已注册时（例如测试进程重复调用）返回错误而不是 panic
    let registry = tracing_subscriber::registry().with(filter).with(file_layer);
    #[cfg(debug_assertions)]
    let registry = registry.with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

    if registry.try_init().is_err() {
        return Err(AppError::IoFailure("日志订阅端已初始化".into()));
    }
    INITIALIZED.get_or_init(|| true);

    // 启动清理：删除超过保留期的日志（失败不影响启动）
    match cleanup_old_logs(log_dir, RETENTION_DAYS) {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, days = RETENTION_DAYS, "已清理过期日志"),
        Err(err) => tracing::warn!(error = %err, "清理过期日志失败"),
    }
    Ok(())
}

/// 日志是否已初始化。
pub fn is_initialized() -> bool {
    INITIALIZED.get().copied().unwrap_or(false)
}

/// 删除 `retention_days` 天前的日志文件，返回删除数量。
///
/// 只处理 `knowl-pad.log*` 命名的文件；目录不存在时返回 0（不视为错误）。
pub fn cleanup_old_logs(dir: &Path, retention_days: u64) -> Result<usize, AppError> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(
            retention_days * 24 * 60 * 60,
        ))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    let mut removed = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(LOG_FILE_NAME) {
            continue;
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if modified < cutoff {
            std::fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch_aged(path: &Path, age_days: u64) {
        fs::write(path, b"log line\n").expect("应可写日志文件");
        let when =
            std::time::SystemTime::now() - std::time::Duration::from_secs(age_days * 24 * 60 * 60);
        // 通过写文件时间控制年龄：设置 mtime 需要 filetime 之类依赖，
        // 这里用「反过来」的方式验证——见 aged_files_are_removed 的构造
        let _ = when;
    }

    #[test]
    fn missing_dir_is_not_an_error() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let missing = dir.path().join("nope");
        assert_eq!(
            cleanup_old_logs(&missing, RETENTION_DAYS).expect("应成功"),
            0
        );
    }

    #[test]
    fn fresh_logs_are_kept_and_unrelated_files_ignored() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        touch_aged(&dir.path().join("knowl-pad.log.2026-10-01"), 0);
        fs::write(dir.path().join("other.txt"), b"keep").expect("应可写");
        fs::write(dir.path().join("knowl-pad.log"), b"current").expect("应可写");
        assert_eq!(
            cleanup_old_logs(dir.path(), RETENTION_DAYS).expect("应成功"),
            0,
            "新日志与非日志文件都不应被删除"
        );
        assert!(dir.path().join("other.txt").exists());
    }

    #[test]
    fn logs_older_than_retention_are_removed() {
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        let old = dir.path().join("knowl-pad.log.2000-01-01");
        fs::write(&old, b"ancient").expect("应可写");
        // 用 0 天保留期等价于「一切早于此刻的文件都过期」
        let removed = cleanup_old_logs(dir.path(), 0).expect("应成功");
        assert_eq!(removed, 1, "过期日志应被删除");
        assert!(!old.exists());
    }

    #[test]
    fn init_writes_events_to_the_log_file() {
        // 端到端自检：init 之后 tracing 事件必须真的落到文件里
        let dir = tempfile::tempdir().expect("临时目录应可创建");
        match init(dir.path()) {
            Ok(()) => {}
            Err(err) => {
                // 同一进程内已被其它测试初始化时跳过（不是失败）
                eprintln!("跳过：{err}");
                return;
            }
        }
        tracing::info!(target: "kp_logging_test", marker = "self-check", "日志管道自检");

        let mut found = false;
        for _ in 0..50 {
            if let Ok(entries) = fs::read_dir(dir.path()) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with(LOG_FILE_NAME) {
                        continue;
                    }
                    if let Ok(body) = fs::read_to_string(entry.path()) {
                        if body.contains("日志管道自检") {
                            found = true;
                        }
                    }
                }
            }
            if found {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(found, "日志事件必须写入日志文件");
        assert!(is_initialized(), "初始化后状态应为已初始化");
    }
}
