//! 改写落地的判据（测试集先行）：**全有或全无**，且任何时刻不出现半改写的文件。

use std::fs;
use std::path::Path;

use kp_domain::link_rewrite_apply::{apply, plan_rename, rename_with_rewrite};

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("建目录");
    }
    fs::write(path, content).expect("写文件");
}

fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).expect("读文件")
}

fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().join("vault");
    let backups = dir.path().join("backups");
    fs::create_dir_all(&root).expect("建 vault");
    write(&root, "a.md", "甲见 [[A]] 与 [[A|别名]]\n");
    write(&root, "b.md", "乙见 [[A#标题]]\n");
    write(&root, "c.md", "丙没有任何链接\n");
    (dir, root, backups)
}

/// 正常路径：两个文件都改到，备份都在，未命中的文件不动。
#[test]
fn apply_writes_all_and_keeps_backups() {
    let (_dir, root, backups) = setup();
    let rels = vec!["a.md".to_string(), "b.md".to_string(), "c.md".to_string()];
    let plan = plan_rename(&root, "A.md", "B.md", &rels).expect("生成计划");
    assert_eq!(plan.changes.len(), 2, "只有 a/b 命中，c 不应进计划");
    assert_eq!(plan.total_hits(), 3, "a 有两处、b 有一处");

    let report = apply(&root, &plan, &backups).expect("落地");
    assert_eq!(report.files, 2);
    assert_eq!(report.hits, 3);
    assert!(read(&root, "a.md").contains("[[B]]") && read(&root, "a.md").contains("[[B|别名]]"));
    assert!(read(&root, "b.md").contains("[[B#标题]]"));
    assert_eq!(
        read(&root, "c.md"),
        "丙没有任何链接\n",
        "未命中的文件不得改动"
    );

    // 断言必须与 read_dir 的返回顺序**无关**：CI 上这里先返回了 b.md 的备份，
    // 而 b 的原文是 [[A#标题]]（不含 [[A]] 字面量），本地恰好顺序相反 —— 靠运气的断言不算断言。
    let mut backups: Vec<String> = fs::read_dir(&report.backup_dir)
        .expect("读备份目录")
        .filter_map(|e| e.ok())
        .map(|e| fs::read_to_string(e.path()).expect("读备份"))
        .collect();
    backups.sort();
    let mut expected = vec![
        "甲见 [[A]] 与 [[A|别名]]\n".to_string(),
        "乙见 [[A#标题]]\n".to_string(),
    ];
    expected.sort();
    assert_eq!(backups, expected, "备份集合必须**恰好**是改写前的两份原文");
}

/// **全有或全无**：第二个文件落地失败时，第一个文件必须被还原成原文。
#[test]
fn failure_midway_rolls_back_everything() {
    let (_dir, root, backups) = setup();
    let rels = vec!["a.md".to_string(), "b.md".to_string()];
    let plan = plan_rename(&root, "A.md", "B.md", &rels).expect("生成计划");
    assert_eq!(plan.changes.len(), 2);

    // 制造"第二个文件写不进去"：把 b.md 换成同名目录
    let b = root.join("b.md");
    fs::remove_file(&b).expect("删 b");
    fs::create_dir(&b).expect("把 b 变成目录");

    let err = apply(&root, &plan, &backups).expect_err("必须失败");
    assert!(
        err.to_string().contains("回滚"),
        "错误信息要说明已回滚：{err}"
    );

    assert_eq!(
        read(&root, "a.md"),
        "甲见 [[A]] 与 [[A|别名]]\n",
        "已写的文件必须被还原成原文（不得留下半完成状态）"
    );
    assert!(b.is_dir(), "失败的那个位置保持原样");
}

/// 空计划：不建备份目录、不写任何文件。
#[test]
fn empty_plan_writes_nothing() {
    let (_dir, root, backups) = setup();
    let plan = plan_rename(&root, "不存在的笔记", "B.md", &["c.md".to_string()]).expect("生成计划");
    assert!(plan.is_empty());
    let report = apply(&root, &plan, &backups).expect("落地空计划");
    assert_eq!(report.files, 0);
    assert!(!backups.exists(), "空计划不应产生备份目录");
    assert_eq!(read(&root, "c.md"), "丙没有任何链接\n");
}

/// **FR-FILE-22 的联合原子性**：重命名与改写同属一次操作，严禁出现
/// 「链接已指向新名、文件仍旧名」的中间态。
mod joint {
    use super::*;

    fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("临时目录");
        let root = dir.path().join("vault");
        let backups = dir.path().join("backups");
        fs::create_dir_all(&root).expect("建 vault");
        write(&root, "A.md", "# 甲\n");
        write(&root, "one.md", "见 [[A]]\n");
        write(&root, "two.md", "也见 [[A|别名]]\n");
        (dir, root, backups)
    }

    /// ① 正常路径：改名成功、链接全部改写、备份都在。
    #[test]
    fn success_renames_and_rewrites_together() {
        let (_d, root, backups) = setup();
        let rels = vec!["one.md".to_string(), "two.md".to_string()];
        let (report, new_rel) =
            rename_with_rewrite(&root, "A.md", "B.md", &rels, &backups).expect("联合操作应成功");
        assert_eq!(new_rel, "B.md");
        assert_eq!(report.files, 2, "两个文件各有一处链接");
        assert!(root.join("B.md").exists(), "文件应已改名");
        assert!(!root.join("A.md").exists(), "旧名不应再存在");
        assert!(read(&root, "one.md").contains("[[B]]"));
        assert!(read(&root, "two.md").contains("[[B|别名]]"), "别名必须保留");
    }

    /// ② **改写失败 → 撤销改名**（FR-FILE-22 的明文要求）。
    #[test]
    #[allow(clippy::permissions_set_readonly_false)] // 见下方注释：仅还原夹具自己设的只读标记
    fn rewrite_failure_undoes_the_rename() {
        let (_d, root, backups) = setup();
        let rels = vec!["one.md".to_string(), "two.md".to_string()];
        // 让第二个被改写目标写不进去：置为**只读**（换成目录不行 —— 目录会让
        // plan_rename 的"读不到就跳过"策略把它滤掉，计划里就没有它了）。
        let two = root.join("two.md");
        let mut perm = fs::metadata(&two).expect("读权限").permissions();
        perm.set_readonly(true);
        fs::set_permissions(&two, perm).expect("置只读");

        let err = rename_with_rewrite(&root, "A.md", "B.md", &rels, &backups)
            .expect_err("改写失败时整体必须失败");
        let msg = err.to_string();
        assert!(msg.contains("撤销改名"), "错误信息要说明已撤销改名：{msg}");
        assert!(root.join("A.md").exists(), "文件必须回到旧名");
        assert!(!root.join("B.md").exists(), "不得停在新名上");
        assert_eq!(read(&root, "one.md"), "见 [[A]]\n", "已改写的文件必须还原");
        // 复原只读标记，便于临时目录清理。
        // clippy::permissions_set_readonly_false：这里只是把**测试夹具**自己刚设上的标记还原，
        // 不涉及"把用户的只读文件改成可写"，因此是本测试内的窄豁免。
        let mut perm = fs::metadata(&two).expect("读权限").permissions();
        perm.set_readonly(false);
        let _ = fs::set_permissions(&two, perm);
    }

    /// ③ **改名失败 → 一处链接都不改写**。
    #[test]
    fn rename_failure_touches_no_link() {
        let (_d, root, backups) = setup();
        let rels = vec!["one.md".to_string(), "two.md".to_string()];
        // 让目标目录无法创建：把 B 的父路径做成一个普通文件
        write(&root, "blocked", "我是文件不是目录\n");
        let err = rename_with_rewrite(&root, "A.md", "blocked/B.md", &rels, &backups)
            .expect_err("改名失败时整体必须失败");
        let _ = err;
        assert!(root.join("A.md").exists(), "源文件应原样保留");
        assert_eq!(read(&root, "one.md"), "见 [[A]]\n", "链接一处都不应改写");
        assert_eq!(
            read(&root, "two.md"),
            "也见 [[A|别名]]\n",
            "链接一处都不应改写"
        );
        assert!(!backups.exists(), "改名都没成功，不应产生备份目录");
    }
}
