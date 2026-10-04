//! file_tree 的单元测试（CODE-11：测试位于独立文件）。

use super::*;
use std::fs;

/// 建一个含嵌套目录、隐藏项与多类型文件的 Vault。
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join("notes/deep")).expect("应可建目录");
    fs::create_dir_all(root.join(".knowlpad")).expect("应可建内部目录");
    fs::create_dir_all(root.join(".obsidian")).expect("应可建第三方目录");
    fs::create_dir_all(root.join(".git")).expect("应可建 VCS 目录");
    fs::create_dir_all(root.join("empty-dir")).expect("应可建空目录");
    fs::write(root.join("notes/a.md"), b"# A").expect("应可写");
    fs::write(root.join("B.md"), b"# B").expect("应可写");
    fs::write(root.join("image.png"), b"png").expect("应可写");
    fs::write(root.join("doc.pdf"), b"pdf").expect("应可写");
    fs::write(root.join("clip.mp4"), b"mp4").expect("应可写");
    fs::write(root.join("data.txt"), b"txt").expect("应可写");
    fs::write(root.join(".knowlpad/index.db"), b"db").expect("应可写");
    fs::write(root.join(".obsidian/app.json"), b"{}").expect("应可写");
    fs::write(root.join(".gitignore"), b"x").expect("应可写");
    fs::write(root.join(".hidden.md"), b"hidden").expect("应可写");
    (dir, root)
}

fn names(entries: &[FileEntry]) -> Vec<String> {
    entries.iter().map(|e| e.name.clone()).collect()
}

#[test]
fn lists_only_one_level() {
    let (_d, root) = fixture();
    let entries = list_dir(&root, "", false).expect("列根目录应成功");
    let got = names(&entries);
    assert!(got.contains(&"notes".to_string()), "应含一级目录");
    assert!(
        !got.iter().any(|n| n == "a.md"),
        "不得返回更深层文件（单层约束）"
    );
    let nested = list_dir(&root, "notes", false).expect("列子目录应成功");
    assert_eq!(
        names(&nested),
        vec!["deep".to_string(), "a.md".to_string()],
        "目录优先 + 名称序"
    );
}

#[test]
fn knowlpad_is_always_hidden_others_follow_the_flag() {
    // AC-FILE-09 逐条对应
    let (_d, root) = fixture();
    let hidden = list_dir(&root, "", false).expect("默认模式应成功");
    let got = names(&hidden);
    assert!(
        !got.contains(&".knowlpad".to_string()),
        ".knowlpad 永远隐藏"
    );
    assert!(
        !got.contains(&".obsidian".to_string()),
        "默认隐藏 .obsidian"
    );
    assert!(!got.contains(&".git".to_string()), "默认隐藏 .git");
    assert!(!got.contains(&".gitignore".to_string()), "默认隐藏点文件");

    let shown = list_dir(&root, "", true).expect("显示隐藏应成功");
    let got2 = names(&shown);
    assert!(
        !got2.contains(&".knowlpad".to_string()),
        "开启显示后 .knowlpad 仍必须隐藏"
    );
    assert!(
        got2.contains(&".obsidian".to_string()),
        "开启后 .obsidian 应可见"
    );
    assert!(got2.contains(&".git".to_string()), "开启后 .git 应可见");
    assert!(
        got2.contains(&".gitignore".to_string()),
        "开启后点文件应可见"
    );
}

#[test]
fn sorts_dirs_first_then_case_insensitive_name() {
    let (_d, root) = fixture();
    let entries = list_dir(&root, "", false).expect("列目录应成功");
    let first_file_idx = entries.iter().position(|e| !e.is_dir).expect("应有文件");
    assert!(
        entries[..first_file_idx].iter().all(|e| e.is_dir),
        "目录必须排在文件之前"
    );
    let files: Vec<&str> = entries
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.name.as_str())
        .collect();
    let mut sorted = files.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert_eq!(files, sorted, "文件应按名称（忽略大小写）排序");
}

#[test]
fn classifies_kinds_by_extension() {
    let (_d, root) = fixture();
    let entries = list_dir(&root, "", false).expect("列目录应成功");
    let kind_of = |name: &str| entries.iter().find(|e| e.name == name).map(|e| e.kind);
    assert_eq!(kind_of("B.md"), Some(FileKind::Note));
    assert_eq!(kind_of("image.png"), Some(FileKind::Attachment));
    assert_eq!(kind_of("doc.pdf"), Some(FileKind::Attachment));
    assert_eq!(kind_of("clip.mp4"), Some(FileKind::Attachment));
    assert_eq!(kind_of("data.txt"), Some(FileKind::Other));
    assert_eq!(classify("MD"), FileKind::Note, "扩展名比较应忽略大小写");
    assert_eq!(classify(""), FileKind::Other);
}

#[test]
fn has_children_reflects_visible_content_only() {
    let (_d, root) = fixture();
    let entries = list_dir(&root, "", false).expect("列目录应成功");
    let by_name = |name: &str| entries.iter().find(|e| e.name == name).cloned();
    assert_eq!(
        by_name("notes").expect("notes 应存在").has_children,
        Some(true)
    );
    assert_eq!(
        by_name("empty-dir").expect("空目录应存在").has_children,
        Some(false)
    );
    assert_eq!(
        by_name("B.md").expect("文件应存在").has_children,
        None,
        "文件无该字段"
    );

    // .obsidian 内只有 app.json：默认模式下属"不可见子项"→ false；开启显示 → true
    let shown = list_dir(&root, "", true).expect("显示隐藏应成功");
    let obsidian = shown
        .iter()
        .find(|e| e.name == ".obsidian")
        .expect(".obsidian 应可见");
    assert_eq!(obsidian.has_children, Some(true));
}

#[test]
fn stat_returns_metadata_and_rejects_missing() {
    let (_d, root) = fixture();
    let entry = stat(&root, "notes/a.md").expect("stat 应成功");
    assert_eq!(entry.rel_path, "notes/a.md");
    assert!(!entry.is_dir);
    assert_eq!(entry.kind, FileKind::Note);
    assert!(entry.size_bytes.unwrap_or(0) > 0);
    assert!(entry.mtime_ms.unwrap_or(0) > 0);
    assert!(stat(&root, "notes").expect("目录 stat 应成功").is_dir);
    assert_eq!(
        stat(&root, "nope.md").expect_err("缺失文件应报错").code(),
        "E_FILE_NOT_FOUND"
    );
}

#[test]
fn rejects_path_escape_in_list_and_stat() {
    let (_d, root) = fixture();
    assert!(
        list_dir(&root, "../outside", false).is_err(),
        "穿越目录必须被拒绝"
    );
    assert!(
        list_dir(&root, "/etc", false).is_err(),
        "绝对路径必须被拒绝"
    );
    assert!(stat(&root, "../secret.md").is_err(), "穿越路径必须被拒绝");
    assert!(
        list_dir(&root, "notes/a.md", false).is_err(),
        "对文件调用 list_dir 应报错"
    );
}

#[test]
fn validate_name_matches_ac_file_03() {
    // AC-FILE-03：CON / a/b / 名称. / na*me / 空格开头 → 全部拒绝且给出具体原因
    for payload in ["CON", "a/b", "名称.", "na*me", "  空格开头"] {
        let result = validate_name(payload);
        assert!(!result.valid, "{payload} 应被拒绝");
        let reason = result.reason.expect("必须给出原因");
        assert!(!reason.is_empty(), "原因不得为空");
        assert!(
            reason
                .chars()
                .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
            "原因应为中文可操作提示：{reason}"
        );
    }
    for ok in ["note.md", "我的笔记.md", "a.b.c.md", "2026-10-02 日记.md"] {
        assert!(validate_name(ok).valid, "{ok} 应合法");
    }
    assert!(
        !validate_name("COM\u{00b9}").valid,
        "Windows 设备别名应被拒绝"
    );
}

#[test]
fn symlinks_are_not_followed() {
    let (dir, root) = fixture();
    let outside = tempfile::tempdir().expect("外部目录应可创建");
    fs::write(outside.path().join("secret.md"), b"x").expect("应可写");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), root.join("link-out")).expect("应可建软链");
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("link-out"))
            .arg(outside.path())
            .output();
        if !matches!(&status, Ok(o) if o.status.success()) {
            eprintln!("跳过：无法创建 junction");
            return;
        }
    }
    let entries = list_dir(&root, "", false).expect("列目录应成功");
    assert!(
        !entries.iter().any(|e| e.name == "link-out"),
        "符号链接/联接不得出现在文件树中"
    );
    drop(dir);
}
