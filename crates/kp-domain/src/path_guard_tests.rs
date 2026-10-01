//! PathGuard 七步校验的单元测试
//! （CODE-11：测试位于独立文件，不占用实现文件行数上限）

use super::*;
use std::fs;

fn guard() -> (tempfile::TempDir, PathGuard) {
    let dir = tempfile::tempdir().unwrap();
    let guard = PathGuard::new(dir.path()).unwrap();
    (dir, guard)
}

#[test]
fn accepts_plain_and_nested_paths() {
    let (dir, guard) = guard();
    fs::create_dir_all(dir.path().join("a/b")).unwrap();
    let p = guard.resolve("a/b/note.md").unwrap();
    assert!(p.ends_with("note.md"));
    assert!(p.starts_with(guard.canonical_root()));
}

#[test]
fn rejects_empty() {
    let (_d, g) = guard();
    assert_eq!(g.resolve("").unwrap_err().code(), "E_PATH_ESCAPE_DENY");
}

#[test]
fn rejects_absolute_and_drive() {
    let (_d, g) = guard();
    assert_eq!(
        g.resolve("/etc/passwd").unwrap_err().code(),
        "E_PATH_OUTSIDE_VAULT"
    );
    assert_eq!(
        g.resolve("C:/Windows/x").unwrap_err().code(),
        "E_PATH_OUTSIDE_VAULT"
    );
}

#[test]
fn rejects_parent_and_curdir() {
    let (_d, g) = guard();
    assert_eq!(
        g.resolve("../x").unwrap_err().code(),
        "E_PATH_OUTSIDE_VAULT"
    );
    assert_eq!(
        g.resolve("a/../../x").unwrap_err().code(),
        "E_PATH_OUTSIDE_VAULT"
    );
    assert_eq!(g.resolve("./x").unwrap_err().code(), "E_PATH_ESCAPE_DENY");
}

#[test]
fn rejects_nul_byte() {
    let (_d, g) = guard();
    let evil = String::from_iter(['a', '\u{0}', 'b']);
    assert_eq!(g.resolve(&evil).unwrap_err().code(), "E_PATH_ESCAPE_DENY");
}

#[test]
fn rejects_invalid_filenames() {
    let (_d, g) = guard();
    let bad = [
        "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "name.", " name", "CON", "com1.md", "..", "...",
    ];
    for item in bad {
        assert!(g.resolve(item).is_err(), "{item} should be rejected");
    }
}

#[test]
fn allows_dotted_and_reserved_like_names() {
    validate_segment("note.md").unwrap();
    validate_segment("a.b.c").unwrap();
    validate_segment("console").unwrap();
    validate_segment(".gitignore").unwrap();
}

#[test]
fn rejects_long_and_control() {
    assert!(validate_segment(&"x".repeat(201)).is_err());
    let ctrl = String::from_iter(['a', '\u{7}', 'b']);
    assert!(validate_segment(&ctrl).is_err());
}

#[test]
fn rejects_colon_segment() {
    let (_d, g) = guard();
    assert!(g.resolve("a:b").is_err());
}

#[test]
fn resolves_nonexistent_nested_file() {
    let (_d, g) = guard();
    let p = g.resolve("new/dir/file.md").unwrap();
    assert!(p.to_string_lossy().ends_with("file.md"));
}

#[test]
fn lexical_normalize_removes_dots() {
    let p = lexical_normalize(Path::new("a/./b/../c"));
    assert_eq!(p.to_string_lossy().replace('\\', "/"), "a/c");
}

#[test]
fn canonical_root_has_no_verbatim_prefix() {
    // NFR-PLAT-10 / 技术方案 §3.5.2：进入 VaultInfo.root 与注册表的路径必须是可读形式
    let (_dir, guard) = guard();
    let root = guard.canonical_root().to_string_lossy().to_string();
    assert!(
        !root.starts_with(r"\\?\"),
        "规范化根不得带 verbatim 前缀：{root}"
    );
    let resolved = guard.resolve("a.md").expect("正常路径应放行");
    assert!(
        !resolved.to_string_lossy().starts_with(r"\\?\"),
        "解析结果不得带 verbatim 前缀：{}",
        resolved.display()
    );
}

#[test]
fn guard_errors_when_root_missing() {
    let missing = std::env::temp_dir().join("kp-definitely-missing-root-xyz");
    assert!(PathGuard::new(&missing).is_err());
}

#[test]
fn windows_separators_are_treated_consistently() {
    // 同一载荷在三平台必须得到**相同判定**：反斜杠按分隔符解析
    let (_d, g) = guard();
    assert_eq!(
        g.resolve("..\\..\\windows\\system32\\config")
            .unwrap_err()
            .code(),
        "E_PATH_OUTSIDE_VAULT"
    );
    // 合法嵌套路径用反斜杠书写时，等价于正斜杠写法
    let a = g.resolve("notes\\a.md").expect("反斜杠嵌套路径应放行");
    let b = g.resolve("notes/a.md").expect("正斜杠嵌套路径应放行");
    assert_eq!(a, b, "两种分隔符必须解析到同一路径");
}

#[test]
fn rejects_percent_encoded_traversal() {
    let (_d, g) = guard();
    // AC-SEC-01 指定的载荷
    for payload in [
        "%2e%2e%2f",
        "%2e%2e%2Fetc",
        "%2E%2E%5Cwindows",
        "%252e%252e%252f",
    ] {
        assert_eq!(
            g.resolve(payload).unwrap_err().code(),
            "E_PATH_ESCAPE_DENY",
            "{payload} 必须被拒绝"
        );
    }
}

#[test]
fn allows_benign_percent_names() {
    // 普通文件名中的百分号不构成穿越，仍应放行
    let (_d, g) = guard();
    for name in [
        "100%2e5.md",
        "50%off.md",
        "report%2E%2Emd", // 解码为 report..md：仍是单段普通名（旧实现误伤）
        "a%2Fb.md",       // 解码为 a/b.md：按原始字面量解析，磁盘上就是这个名字
    ] {
        validate_segment(name).unwrap_or_else(|err| panic!("{name} 应合法：{err}"));
        g.resolve(name)
            .unwrap_or_else(|err| panic!("{name} 应可解析：{err:?}"));
    }
}

#[test]
fn rejects_encoded_traversal_at_path_level() {
    let (_d, g) = guard();
    for payload in [
        "%2e%2e%2f",
        "%2e%2e%2fetc",
        "..%2f..%2fetc",
        "%2E%2E%5Cwindows",
        "%252e%252e%252f",
        "%2e%2e%2f%2e%2e%2fetc",
    ] {
        assert_eq!(
            g.resolve(payload).unwrap_err().code(),
            "E_PATH_ESCAPE_DENY",
            "{payload} 必须被拒绝"
        );
    }
}

#[test]
fn rejects_win32_reserved_aliases() {
    for name in [
        "COM\u{00b9}.md",
        "COM\u{00b2}",
        "LPT\u{00b3}.txt",
        "CONIN$",
        "CONOUT$.md",
    ] {
        assert!(
            validate_segment(name).is_err(),
            "{name} 是 Windows 设备别名，必须拒绝"
        );
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    let outside = tempfile::tempdir().unwrap();
    let (dir, guard) = guard();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    assert_eq!(
        guard.resolve("link/evil.md").unwrap_err().code(),
        "E_PATH_ESCAPE_DENY"
    );
}
