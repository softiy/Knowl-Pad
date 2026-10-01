//! AC-SEC-01｜路径穿越全面拦截（M1 DoD：路径穿越测试全拦截）。
//!
//! 载荷清单直接取自 PRD §6.3 的 AC-SEC-01，并补充常见变体（UNC、编码、全角、保留名）。
//! 每个载荷都必须：① 返回 E_PATH_OUTSIDE_VAULT 或 E_PATH_ESCAPE_DENY；
//! ② 在 Vault 外不产生任何读取/创建/修改/删除副作用。

use kp_domain::path_guard::PathGuard;
use std::fs;
use std::path::{Path, PathBuf};

const ALLOWED_CODES: [&str; 2] = ["E_PATH_OUTSIDE_VAULT", "E_PATH_ESCAPE_DENY"];

/// 建一个 Vault 目录 + 一个「外部哨兵」文件，用于断言无越界副作用。
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, PathGuard) {
    let vault = tempfile::tempdir().expect("Vault 临时目录应可创建");
    let outside = tempfile::tempdir().expect("外部临时目录应可创建");
    let sentinel = outside.path().join("sentinel.txt");
    fs::write(&sentinel, b"PRECIOUS").expect("哨兵文件应可写入");
    let guard = PathGuard::new(vault.path()).expect("Vault 根应可建立校验器");
    (vault, outside, sentinel, guard)
}

/// 越界载荷矩阵：标签 + 载荷（PRD AC-SEC-01 的 6 项以 ★ 标注）。
fn traversal_payloads(outside: &Path) -> Vec<(&'static str, String)> {
    let outside_str = outside.to_string_lossy().to_string();
    vec![
        // ★ PRD AC-SEC-01 指定的 6 个载荷
        ("prd-01 经典穿越", "../../../etc/passwd".to_string()),
        (
            "prd-02 Windows 反斜杠",
            "..\\..\\windows\\system32\\config".to_string(),
        ),
        ("prd-03 相对穿越", "folder/../../outside.md".to_string()),
        ("prd-04 百分号编码", "%2e%2e%2f".to_string()),
        ("prd-05 NUL 字节", "notes/evil\u{0}.md".to_string()),
        ("prd-06 符号链接", "link-to-outside/evil.md".to_string()),
        // 变体
        ("绝对路径 POSIX", "/etc/passwd".to_string()),
        ("绝对路径 Windows", "C:/Windows/System32/config".to_string()),
        ("UNC 路径", "//server/share/file.md".to_string()),
        ("反斜杠 UNC", "\\\\server\\share\\file.md".to_string()),
        ("深层穿越", "a/b/c/../../../../x.md".to_string()),
        ("仅父目录", "..".to_string()),
        ("父目录加文件", "../x.md".to_string()),
        ("当前目录", "./x.md".to_string()),
        ("双重编码穿越", "%252e%252e%252f".to_string()),
        ("编码反斜杠", "%2e%2e%5cwindows".to_string()),
        ("外部绝对路径直传", format!("{outside_str}/evil.md")),
        ("空字符串", String::new()),
    ]
}

/// 文件名合法性载荷：同属「必须拦截」，但语义上返回 E_INVALID_FILENAME 更准确
/// （AC-SEC-01 针对的是穿越语义，故与之分开断言）。
fn filename_payloads() -> Vec<(&'static str, String)> {
    vec![
        ("带冒号段（盘符/ADS 语义）", "notes:secret.md".to_string()),
        ("Windows 保留设备名", "CON.md".to_string()),
        ("结尾点号", "note.".to_string()),
        ("开头空格", " note.md".to_string()),
        ("非法字符星号", "a*b.md".to_string()),
        ("超长文件名", "x".repeat(201)),
    ]
}

#[test]
fn ac_sec_01_every_payload_is_rejected() {
    // 下划线前缀：vault 仅在 unix 分支（准备符号链接）使用
    let (_vault, outside, _sentinel, guard) = fixture();
    // 为符号链接载荷准备一个指向 Vault 外的链接
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), _vault.path().join("link-to-outside"))
        .expect("应可创建符号链接");

    for (label, payload) in traversal_payloads(outside.path()) {
        #[cfg(not(unix))]
        if label == "prd-06 符号链接" {
            continue; // Windows 上创建符号链接需要特权，该场景在 unix CI 覆盖
        }
        match guard.resolve(&payload) {
            Ok(resolved) => panic!("{label}（{payload:?}）必须被拒绝，却解析为 {resolved:?}"),
            Err(err) => assert!(
                ALLOWED_CODES.contains(&err.code()),
                "{label}（{payload:?}）应返回 E_PATH_OUTSIDE_VAULT 或 E_PATH_ESCAPE_DENY，实得 {}",
                err.code()
            ),
        }
    }
}

#[test]
fn filename_payloads_are_rejected_too() {
    // 这些不是穿越，但同样必须拦截——且错误码应为 E_INVALID_FILENAME 或穿越类错误码
    let (_vault, _outside, _sentinel, guard) = fixture();
    let allowed = [
        "E_INVALID_FILENAME",
        "E_PATH_OUTSIDE_VAULT",
        "E_PATH_ESCAPE_DENY",
    ];
    for (label, payload) in filename_payloads() {
        let err = guard
            .resolve(&payload)
            .err()
            .unwrap_or_else(|| panic!("{label} 必须被拒绝"));
        assert!(
            allowed.contains(&err.code()),
            "{label}（{payload:?}）错误码应属 {allowed:?}，实得 {}",
            err.code()
        );
    }
}

#[test]
fn ac_sec_01_no_side_effects_outside_vault() {
    let (vault, outside, sentinel, guard) = fixture();
    let before_outside = list_dir(outside.path());
    let before_vault = list_dir(vault.path());

    for (_label, payload) in traversal_payloads(outside.path())
        .into_iter()
        .chain(filename_payloads())
    {
        // 校验失败后不得有任何写入尝试（resolve 本身只读，此处断言目录未变化作为回归保护）
        let _ = guard.resolve(&payload);
    }

    assert_eq!(
        fs::read(&sentinel).expect("哨兵应仍可读"),
        b"PRECIOUS",
        "Vault 外文件不得被修改"
    );
    assert_eq!(
        list_dir(outside.path()),
        before_outside,
        "Vault 外目录不得新增/删除条目"
    );
    assert_eq!(
        list_dir(vault.path()),
        before_vault,
        "被拒绝的载荷不得在 Vault 内留下痕迹"
    );
}

#[test]
fn ac_sec_01_symlink_escape_is_blocked_but_internal_symlink_allowed() {
    let (vault, outside, _sentinel, guard) = fixture();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), vault.path().join("escape")).expect("外部链接");
        let err = guard
            .resolve("escape/evil.md")
            .expect_err("指向外部的符号链接必须被拒绝");
        assert!(ALLOWED_CODES.contains(&err.code()), "实得 {}", err.code());

        // Vault 内部的符号链接（指向 Vault 内）应正常放行
        fs::create_dir_all(vault.path().join("real")).expect("应可建目录");
        fs::write(vault.path().join("real/note.md"), b"# hi").expect("应可写文件");
        std::os::unix::fs::symlink(vault.path().join("real"), vault.path().join("inner"))
            .expect("内部链接");
        let resolved = guard
            .resolve("inner/note.md")
            .expect("Vault 内符号链接应放行");
        assert!(
            resolved.starts_with(guard.canonical_root()),
            "解析结果必须仍在 Vault 内"
        );
    }
    #[cfg(not(unix))]
    {
        // Windows 上创建符号链接需要特权，该场景交由 unix CI 覆盖
        let _ = (vault, outside, guard);
    }
}

#[test]
fn legitimate_paths_still_resolve_inside_vault() {
    // 反向保护：拦截不得误伤正常路径
    let (vault, _outside, _sentinel, guard) = fixture();
    fs::create_dir_all(vault.path().join("notes/2026")).expect("应可建目录");
    for ok in [
        "note.md",
        "notes/n.md",
        "notes/2026/plan.md",
        ".knowlpad/index.db",
    ] {
        let resolved = guard.resolve(ok).expect("正常路径必须放行");
        assert!(
            resolved.starts_with(guard.canonical_root()),
            "{ok} 应解析到 Vault 内"
        );
    }
}

/// 目录条目快照（排序后比较，避免文件系统顺序差异）。
fn list_dir(path: &Path) -> Vec<String> {
    let mut entries: Vec<String> = fs::read_dir(path)
        .expect("目录应可读")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    entries.sort();
    entries
}
