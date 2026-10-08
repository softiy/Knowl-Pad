//! 改写落地的判据（测试集先行）：**全有或全无**，且任何时刻不出现半改写的文件。

use std::fs;
use std::path::Path;

use kp_domain::link_rewrite_apply::{apply, plan_rename};

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
