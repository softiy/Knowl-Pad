//! file_ops 的单元测试（CODE-11：测试位于独立文件）。

use super::*;
use crate::file_trash::{prune_backups, yyyy_mm, BACKUP_KEEP_DEFAULT};
use std::fs;

fn vault() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录应可创建");
    let root = dir.path().to_path_buf();
    (dir, root)
}

#[test]
fn create_note_writes_content_and_reports_mtime() {
    let (_d, root) = vault();
    let out = create_note(&root, "a.md", "# 你好", ConflictPolicy::Cancel).expect("新建应成功");
    assert_eq!(out.rel_path, "a.md");
    assert!(out.new_mtime_ms > 0);
    assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), "# 你好");
}

#[test]
fn create_note_cancel_keeps_original_bytes() {
    // AC-FILE-04：选择「取消」后原文件内容**完全不变**
    let (_d, root) = vault();
    fs::write(root.join("note.md"), "原始内容").unwrap();
    let before = fs::read(root.join("note.md")).unwrap();
    let err = create_note(&root, "note.md", "新内容", ConflictPolicy::Cancel).unwrap_err();
    assert_eq!(err.code(), "E_FILE_EXISTS");
    assert_eq!(
        fs::read(root.join("note.md")).unwrap(),
        before,
        "原文件必须逐字节不变"
    );
    assert!(!root.join(".knowlpad").exists(), "取消不应产生备份目录");
}

#[test]
fn create_note_overwrite_backs_up_original_first() {
    // AC-FILE-04：选择「覆盖」前原文件已备份
    let (_d, root) = vault();
    fs::write(root.join("note.md"), "旧内容").unwrap();
    let out =
        create_note(&root, "note.md", "新内容", ConflictPolicy::Overwrite).expect("覆盖应成功");
    assert_eq!(out.rel_path, "note.md");
    assert_eq!(fs::read_to_string(root.join("note.md")).unwrap(), "新内容");
    let backup_root = root.join(".knowlpad/backup");
    let stamps: Vec<_> = fs::read_dir(&backup_root)
        .expect("应有备份目录")
        .flatten()
        .collect();
    assert_eq!(stamps.len(), 1, "应恰好产生一次备份");
    let copied = stamps[0].path().join("note.md");
    assert_eq!(
        fs::read_to_string(&copied).unwrap(),
        "旧内容",
        "备份必须是**原**内容"
    );
}

#[test]
fn create_note_rename_new_keeps_original_and_picks_suffix() {
    let (_d, root) = vault();
    fs::write(root.join("note.md"), "原有").unwrap();
    let out = create_note(&root, "note.md", "第二份", ConflictPolicy::RenameNew)
        .expect("重命名新建应成功");
    assert_eq!(out.rel_path, "note 1.md", "应追加序号（FR-ATTACH-05 习惯）");
    assert_eq!(fs::read_to_string(root.join("note.md")).unwrap(), "原有");
    assert_eq!(
        fs::read_to_string(root.join("note 1.md")).unwrap(),
        "第二份"
    );
    assert!(
        !root.join(".knowlpad/backup").exists(),
        "重命名新建无需备份"
    );
}

#[test]
fn create_note_rejects_illegal_names_without_touching_disk() {
    // AC-FILE-03：CON / a/b / 名称. / na*me / 空格开头 → 全部拒绝且磁盘上不产生任何文件
    let (_d, root) = vault();
    for payload in ["CON", "a/b", "名称.", "na*me", "  空格开头"] {
        assert!(
            create_note(&root, payload, "x", ConflictPolicy::Cancel).is_err(),
            "{payload} 应被拒绝"
        );
    }
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        0,
        "磁盘上不得产生任何条目"
    );
}

#[test]
fn create_note_into_missing_folder_fails() {
    let (_d, root) = vault();
    let err = create_note(&root, "nope/a.md", "x", ConflictPolicy::Cancel).unwrap_err();
    assert_eq!(err.code(), "E_FILE_NOT_FOUND");
}

#[test]
fn create_folder_is_multi_level_and_idempotent() {
    let (_d, root) = vault();
    create_folder(&root, "a/b/c").expect("多级创建应成功");
    assert!(root.join("a/b/c").is_dir());
    create_folder(&root, "a/b/c").expect("重复创建应幂等");
    fs::write(root.join("file.md"), "x").unwrap();
    assert_eq!(
        create_folder(&root, "file.md").unwrap_err().code(),
        "E_FILE_EXISTS"
    );
}

#[test]
fn rename_moves_file_and_keeps_content() {
    let (_d, root) = vault();
    create_folder(&root, "sub").unwrap();
    fs::write(root.join("a.md"), "# A").unwrap();
    let out = rename_path(&root, "a.md", "sub/b.md", ConflictPolicy::Cancel).expect("移动应成功");
    assert_eq!(out.from, "a.md");
    assert_eq!(out.to, "sub/b.md");
    assert!(!root.join("a.md").exists());
    assert_eq!(fs::read_to_string(root.join("sub/b.md")).unwrap(), "# A");
}

#[test]
fn rename_conflict_paths_behave_like_create() {
    let (_d, root) = vault();
    fs::write(root.join("a.md"), "A").unwrap();
    fs::write(root.join("b.md"), "B").unwrap();
    // 取消
    assert_eq!(
        rename_path(&root, "a.md", "b.md", ConflictPolicy::Cancel)
            .unwrap_err()
            .code(),
        "E_FILE_EXISTS"
    );
    assert_eq!(fs::read_to_string(root.join("b.md")).unwrap(), "B");
    // 覆盖：先备份
    rename_path(&root, "a.md", "b.md", ConflictPolicy::Overwrite).expect("覆盖应成功");
    assert_eq!(fs::read_to_string(root.join("b.md")).unwrap(), "A");
    let backup_dirs: Vec<_> = fs::read_dir(root.join(".knowlpad/backup"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(
        fs::read_to_string(backup_dirs[0].path().join("b.md")).unwrap(),
        "B",
        "被覆盖的文件应先备份"
    );
    // 重命名新建
    fs::write(root.join("c.md"), "C").unwrap();
    let out =
        rename_path(&root, "c.md", "b.md", ConflictPolicy::RenameNew).expect("重命名新建应成功");
    assert_eq!(out.to, "b 1.md");
    assert_eq!(fs::read_to_string(root.join("b.md")).unwrap(), "A");
}

#[test]
fn rename_refuses_self_subtree_and_non_empty_dir_overwrite() {
    let (_d, root) = vault();
    create_folder(&root, "outer/inner").unwrap();
    let err = rename_path(&root, "outer", "outer/inner/outer", ConflictPolicy::Cancel).unwrap_err();
    assert_eq!(err.code(), "E_IO_FAILURE");
    fs::write(root.join("outer/x.md"), "x").unwrap();
    fs::write(root.join("y.md"), "y").unwrap();
    let err2 = rename_path(&root, "y.md", "outer", ConflictPolicy::Overwrite).unwrap_err();
    assert_eq!(err2.code(), "E_FILE_EXISTS", "非空目录不得被覆盖");
    assert!(root.join("outer/x.md").exists());
}

#[test]
fn delete_moves_file_to_trash_and_records_manifest() {
    // FR-FILE-30 + FR-TRASH-02：实体进 trash/<yyyy-MM>/，原始相对路径进 manifest.json
    let (_d, root) = vault();
    create_folder(&root, "notes").unwrap();
    fs::write(root.join("notes/a.md"), "内容").unwrap();
    let out = delete_path(&root, "notes/a.md", false).expect("删除应成功");
    assert_eq!(out.trashed_count, 1);
    assert!(!root.join("notes/a.md").exists(), "原位置应不存在");
    let trashed = root.join(&out.trash_rel_path);
    assert!(
        trashed.is_file(),
        "实体应位于回收站：{}",
        out.trash_rel_path
    );
    assert_eq!(fs::read_to_string(&trashed).unwrap(), "内容");
    let manifest = fs::read_to_string(root.join(".knowlpad/trash/manifest.json")).unwrap();
    assert!(
        manifest.trim_start().starts_with('['),
        "清单应是 JSON 数组：{manifest}"
    );
    assert!(
        manifest.contains("notes/a.md"),
        "清单必须记录原始相对路径：{manifest}"
    );
    assert!(manifest.trim_end().ends_with(']'));
}

#[test]
fn delete_directory_requires_recursive_and_counts_entries() {
    let (_d, root) = vault();
    create_folder(&root, "dir/sub").unwrap();
    fs::write(root.join("dir/a.md"), "a").unwrap();
    fs::write(root.join("dir/sub/b.md"), "b").unwrap();
    let err = delete_path(&root, "dir", false).unwrap_err();
    assert_eq!(err.code(), "E_FILE_EXISTS");
    assert!(root.join("dir/a.md").exists(), "未确认时不得删除任何东西");
    let out = delete_path(&root, "dir", true).expect("递归删除应成功");
    assert_eq!(out.trashed_count, 3, "应统计 2 个文件 + 1 个子目录");
    assert!(!root.join("dir").exists());
    let manifest = fs::read_to_string(root.join(".knowlpad/trash/manifest.json")).unwrap();
    assert!(
        manifest.contains("\"isDir\":true"),
        "目录条目应标记 isDir：{manifest}"
    );
}

#[test]
fn delete_manifest_appends_multiple_entries_as_valid_array() {
    let (_d, root) = vault();
    fs::write(root.join("a.md"), "a").unwrap();
    fs::write(root.join("b.md"), "b").unwrap();
    delete_path(&root, "a.md", false).unwrap();
    delete_path(&root, "b.md", false).unwrap();
    let manifest = fs::read_to_string(root.join(".knowlpad/trash/manifest.json")).unwrap();
    assert_eq!(manifest.matches("originalRelPath").count(), 2);
    let opens = manifest.matches('{').count();
    let closes = manifest.matches('}').count();
    assert_eq!(opens, closes, "花括号必须配对：{manifest}");
    assert!(manifest.contains("a.md") && manifest.contains("b.md"));
}

#[test]
fn delete_missing_path_reports_not_found() {
    let (_d, root) = vault();
    assert_eq!(
        delete_path(&root, "nope.md", false).unwrap_err().code(),
        "E_FILE_NOT_FOUND"
    );
    assert_eq!(
        delete_path(&root, "../x.md", false).unwrap_err().code(),
        "E_PATH_OUTSIDE_VAULT"
    );
}

#[test]
fn prune_backups_keeps_recent_and_never_evicts_newest() {
    // NFR-REL-05：默认保留最近 10 次；最新一次绝不被淘汰
    let (_d, root) = vault();
    let backup = root.join(".knowlpad/backup");
    for stamp in 1..=12 {
        let dir = backup.join(stamp.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("f.md"), "x").unwrap();
    }
    let removed = prune_backups(&root, BACKUP_KEEP_DEFAULT).expect("清理应成功");
    assert_eq!(removed, 2);
    let left: Vec<String> = fs::read_dir(&backup)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(left.len(), 10);
    assert!(left.contains(&"12".to_string()), "最新一次必须在");
    assert!(!left.contains(&"1".to_string()));
}

#[test]
fn month_bucket_format_is_yyyy_mm() {
    assert_eq!(yyyy_mm(0), "1970-01");
    assert_eq!(yyyy_mm(1_700_000_000_000), "2023-11");
    conflict_policy_parse_roundtrip();
}

fn conflict_policy_parse_roundtrip() {
    assert_eq!(
        ConflictPolicy::parse("overwrite").unwrap(),
        ConflictPolicy::Overwrite
    );
    assert_eq!(
        ConflictPolicy::parse("renameNew").unwrap(),
        ConflictPolicy::RenameNew
    );
    assert_eq!(
        ConflictPolicy::parse("cancel").unwrap(),
        ConflictPolicy::Cancel
    );
    assert!(ConflictPolicy::parse("nope").is_err());
}

/// M3 回归：清单**不可读**时必须报错中止删除，绝不能当作空清单再覆盖写
/// （否则下一次删除会把全部历史条目清空，而清单是 FR-TRASH-12 的权威来源）。
#[test]
fn delete_path_aborts_when_manifest_unreadable() {
    let (_d, root) = vault();
    create_note(&root, "a.md", "# 内容", ConflictPolicy::Cancel).expect("新建应成功");
    let manifest = root.join(".knowlpad").join("trash").join("manifest.json");
    fs::create_dir_all(manifest.parent().expect("父目录")).expect("建目录");
    fs::write(&manifest, [0xff, 0xfe, 0x00]).expect("写坏清单");
    let before = fs::read(&manifest).expect("读回");

    let err = delete_path(&root, "a.md", false).unwrap_err();
    assert_eq!(err.code(), "E_IO_FAILURE");
    assert_eq!(fs::read(&manifest).expect("读回"), before, "清单不得被覆盖");
    assert!(root.join("a.md").exists(), "删除中止后原文件必须仍在");
}

/// FR-STORAGE-02（P0）：第三方配置目录与应用内部目录**不得被写、改、删**
/// —— 但读（列目录/打开）不受限，AC-FILE-09 要求它们可见。
#[test]
fn protected_dirs_reject_writes() {
    let (_d, root) = vault();
    fs::create_dir_all(root.join(".obsidian")).expect("造第三方目录");
    fs::write(root.join(".obsidian").join("app.json"), "{}").expect("第三方文件");
    fs::create_dir_all(root.join(".knowlpad")).expect("造内部目录");

    for rel in [".obsidian/new.md", ".git/config", ".knowlpad/x.md"] {
        let err = create_note(&root, rel, "x", ConflictPolicy::Cancel).unwrap_err();
        assert_eq!(err.code(), "E_PATH_ESCAPE_DENY", "{rel} 必须被拒");
        assert!(
            err.to_string().contains("不会修改"),
            "文案需说明原因：{err}"
        );
    }
    assert_eq!(
        delete_path(&root, ".obsidian", true).unwrap_err().code(),
        "E_PATH_ESCAPE_DENY"
    );
    assert_eq!(
        rename_path(&root, ".obsidian", "moved", ConflictPolicy::Cancel)
            .unwrap_err()
            .code(),
        "E_PATH_ESCAPE_DENY"
    );
    assert_eq!(
        create_folder(&root, ".git/sub").unwrap_err().code(),
        "E_PATH_ESCAPE_DENY"
    );
    // 第三方目录内容逐字节不变
    assert_eq!(
        fs::read_to_string(root.join(".obsidian").join("app.json")).unwrap(),
        "{}"
    );
}

/// 保存**不得**让已删除的文件复活（回收站清单与磁盘必须一致）。
#[test]
fn save_does_not_resurrect_deleted_note() {
    let (_d, root) = vault();
    create_folder(&root, "dir").expect("建目录");
    create_note(&root, "dir/a.md", "# 内容", ConflictPolicy::Cancel).expect("新建");
    let target = root.join("dir").join("a.md");
    delete_path(&root, "dir", true).expect("删除目录");
    assert!(!target.exists());

    // 模拟"标签仍打开时的自动保存"：直接对已删除路径写入
    let err = crate::note_io::write_note(&target, b"resurrect", None).unwrap_err();
    assert_eq!(err.code(), "E_FILE_NOT_FOUND");
    assert!(!target.exists(), "已删除的文件不得被写回");
}

/// **DEBT-16 残余的回归**：护栏必须可被命令层复用（`note_write` 此前绕过它）。
#[test]
fn ensure_writable_is_public_for_command_layer() {
    for rejected in [
        ".knowlpad/index.db",
        ".KNOWLPAD/index.db",
        ".obsidian/app.json",
        ".git/config",
    ] {
        assert!(
            crate::file_ops::ensure_writable(rejected).is_err(),
            "{rejected} 必须被拒绝"
        );
    }
    // a/.knowlpad/x 是**刻意允许**的：vault_paths 只把**根级**受保护目录视作禁区
    // （文档写明：嵌套的 .knowlpad 是用户的普通目录）。
    for allowed in ["note.md", "dir/note.md", ".hidden-note.md", "a/.knowlpad/x"] {
        assert!(
            crate::file_ops::ensure_writable(allowed).is_ok(),
            "{allowed} 不应被拒绝"
        );
    }
}
