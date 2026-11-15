use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use serde::Serialize;

/// 最小连通性探测（M0 DoD：invoke 往返成功）。
#[tauri::command]
pub async fn ping() -> String {
    "pong".to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub platform: String,
    pub arch: String,
    pub version: String,
    /// **FR-GLOBAL-01 / DEBT-20**：本次启动是否因全局库不可用而重建（正常启动恒 `false`）。
    pub global_db_rebuilt: bool,
    /// 原库被保留成的**文件名**（不含目录；未保留/未重建时为 None）。
    pub global_db_backup: Option<String>,
    /// 重建原因摘要（中文可读；正常启动为 None）。
    pub global_db_reason: Option<String>,
}

/// 平台/版本信息（PRD §5.3.9）。
///
/// **2026-10-21（DEBT-20）**：顺带承担"全局库已重建"的一次性告知 —— 恢复信息只在启动时产生一次，
/// 前端在布局挂载时取一次即可，不需要新事件（事件可能在 webview 就绪前发出而丢失）。
// 同步命令：读取内存里的恢复标记，没有 IO（async + `State<'_>` 会被 Tauri 要求返回 Result，
// 而这里"取不到"不是错误 —— 保持无错误的同步签名更贴合语义）。
#[tauri::command]
pub fn system_info(state: tauri::State<'_, AppState>) -> SystemInfo {
    let recovery = state.global_recovery();
    SystemInfo {
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        global_db_rebuilt: recovery.is_some(),
        global_db_backup: recovery.as_ref().and_then(|r| r.preserved_file.clone()),
        global_db_reason: recovery.map(|r| r.reason),
    }
}

/// **E2 / FR-ATTACH-11 / SEC-07 / SEC-08**：允许交给系统默认浏览器的 scheme 白名单。
///
/// 为什么必须在这里（可信侧）过滤：URL 来自笔记正文（不可信输入），而
/// `tauri_plugin_opener::open_url` 会把任意 scheme 交给操作系统 —— 包括 `file:`
/// （等于绕过 R-01 的路径封装）以及注册表里的自定义 handler。只放行浏览器类协议。
const ALLOWED_EXTERNAL_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// 外部 URL 长度上限（浏览器地址栏量级；超长 URL 不是我们要支持的用户意图）。
const MAX_EXTERNAL_URL_LEN: usize = 2048;

/// 校验一个待打开的外部 URL；返回去首尾空白后的原文。
///
/// 拒绝口径（任一命中即 `E_PATH_ESCAPE_DENY` 类错误）：
/// - 空串、超长、含**控制字符/空白**（避免把参数拼进命令行式的执行链）；
/// - 没有 scheme、scheme 不在 {@link ALLOWED_EXTERNAL_SCHEMES} 内；
/// - `http(s)` 缺 `//host`、`mailto` 后没有地址（避免把"看起来像 URL 的普通文本"交出去）。
///
/// 复用 `E_PATH_ESCAPE_DENY`（403 / "提示操作非法"）而不是新增 PRD §5.2 错误码：
/// 语义都是"拒绝把不可信引用带出应用边界"，前端的处理也一致（可见降级提示）。
pub fn checked_external_url(raw: &str) -> Result<&str, AppError> {
    let url = raw.trim();
    if url.is_empty() || url.len() > MAX_EXTERNAL_URL_LEN {
        return Err(AppError::PathEscapeDeny);
    }
    if url.chars().any(char::is_control) || url.chars().any(char::is_whitespace) {
        return Err(AppError::PathEscapeDeny);
    }
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(AppError::PathEscapeDeny);
    };
    let allowed = ALLOWED_EXTERNAL_SCHEMES
        .iter()
        .any(|candidate| scheme.eq_ignore_ascii_case(candidate));
    if !allowed {
        return Err(AppError::PathEscapeDeny);
    }
    if scheme.eq_ignore_ascii_case("mailto") {
        if rest.is_empty() {
            return Err(AppError::PathEscapeDeny);
        }
    } else if !rest.starts_with("//") || rest.len() <= 2 {
        // rest.len() <= 2 就是 "//" 之后没有 host：`https://` 不算可打开的链接
        return Err(AppError::PathEscapeDeny);
    }
    Ok(url)
}

/// 用系统默认浏览器打开一个外部链接（E2；**应用进程零网络请求** —— 请求由浏览器发起，SEC-08）。
///
/// 命令是薄壳（RS-01）：白名单校验 → 调 opener 插件 → 映射错误。
#[tauri::command]
pub async fn open_external(url: String) -> Result<(), KpError> {
    let target = checked_external_url(&url).map_err(KpError)?;
    tauri_plugin_opener::open_url(target, None::<&str>).map_err(|err| {
        // 不记录 URL 本身（来自笔记正文，SEC-09 的同一口径），只记录底层错误
        tracing::warn!(error = %err, "打开外部链接失败");
        KpError(AppError::IoFailure(
            "无法用系统默认浏览器打开该链接。请检查系统默认浏览器设置后重试。".to_string(),
        ))
    })
}

#[cfg(test)]
mod external_url_tests {
    use super::*;

    #[test]
    fn allows_browser_schemes() {
        for url in [
            "https://example.com/a.png",
            "http://example.com/a.png",
            "HTTPS://EXAMPLE.COM/A.PNG",
            "mailto:user@example.com",
        ] {
            // AppError 未实现 PartialEq（领域错误不该靠相等比较），用 matches! 断言成功值
            assert!(
                matches!(checked_external_url(url), Ok(value) if value == url),
                "{url} 应被放行"
            );
        }
        // 首尾空白被裁掉（粘贴来的 URL 常见）
        assert_eq!(
            checked_external_url("  https://example.com/x  ").ok(),
            Some("https://example.com/x")
        );
    }

    #[test]
    fn rejects_dangerous_or_malformed_schemes() {
        for url in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "data:text/html,<script>alert(1)</script>",
            "asset://localhost/C:/Windows/x.png",
            "ftp://example.com/a.png",
            "vbscript:msgbox(1)",
            // 没有 scheme / 协议相对：前端也可能误传相对地址
            "example.com/a.png",
            "//example.com/a.png",
            "",
            "   ",
            // 形态不完整
            "https://",
            "https:/example.com/a.png",
            "mailto:",
            // 控制字符 / 空白（换行注入）
            "https://example.com/a
rm -rf /",
            "https://example.com/a b.png",
        ] {
            assert!(
                checked_external_url(url).is_err(),
                "{url:?} 不应被放行（scheme 白名单是 SEC-07 的边界）"
            );
        }
    }

    #[test]
    fn rejects_overlong_url() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_EXTERNAL_URL_LEN));
        assert!(checked_external_url(&long).is_err(), "超长 URL 应被拒绝");
    }
}
