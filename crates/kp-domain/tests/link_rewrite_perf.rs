//! NFR-PERF-16：**重命名 + 链接改写（300 处引用）< 3s / 5s / 15s**。
//!
//! 按 M4 计划 §8 的做法：**给宽松上限 + 打印实测**（CI 机器比开发机慢，硬编码 3s 会变成 flaky；
//! 真正的回归靠打印值与基线对比，而不是让用例在慢机器上红）。
//!
//! 300 处引用分布到 300 个文件（更接近真实：一次改名会牵动大量文件）。

use std::fs;
use std::time::Instant;

use kp_domain::link_rewrite_apply::{apply, plan_rename, rename_with_rewrite};

/// 生成 300 个各含一处 `[[A]]` 的笔记 + 目标文件 A.md。
fn setup(root: &std::path::Path) -> Vec<String> {
    let mut rels = Vec::new();
    for i in 0..300 {
        let rel = format!("notes/n{i:03}.md");
        let path = root.join(&rel);
        fs::create_dir_all(path.parent().unwrap()).expect("建目录");
        fs::write(&path, format!("# 笔记 {i}\n\n见 [[A]] 与其它内容\n")).expect("写文件");
        rels.push(rel);
    }
    fs::write(root.join("A.md"), "# 甲\n").expect("写目标");
    rels
}

/// 预览 + 落地 300 处：宽松上限 15s，并把实测毫秒数打印出来供与基线对比。
#[test]
fn nfr_perf_16_rewrites_three_hundred_references_quickly() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let backups = dir.path().join("backups");
    let rels = setup(&root);

    let t0 = Instant::now();
    let plan = plan_rename(&root, "A.md", "B.md", &rels).expect("生成计划");
    let plan_ms = t0.elapsed().as_millis();
    assert_eq!(plan.changes.len(), 300, "300 个文件都要命中");
    assert_eq!(plan.total_hits(), 300, "共 300 处");

    let t1 = Instant::now();
    let report = apply(&root, &plan, &backups).expect("落地");
    let apply_ms = t1.elapsed().as_millis();
    assert_eq!(report.files, 300);

    println!(
        "NFR-PERF-16 实测：计划 {plan_ms} ms + 落地 {apply_ms} ms = {} ms（阈值 3s/5s/15s）",
        plan_ms + apply_ms
    );
    assert!(
        plan_ms + apply_ms < 15_000,
        "300 处改写的总耗时不得超过最宽档 15s（实测 {} ms）",
        plan_ms + apply_ms
    );
    // 抽查首尾两个文件确实被改到
    assert!(fs::read_to_string(root.join("notes/n000.md"))
        .unwrap()
        .contains("[[B]]"));
    assert!(fs::read_to_string(root.join("notes/n299.md"))
        .unwrap()
        .contains("[[B]]"));
}

/// 联合操作（改名 + 改写）在同样规模下的实测（FR-FILE-22 的路径）。
#[test]
fn nfr_perf_16_rename_with_rewrite_scales() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let backups = dir.path().join("backups");
    let rels = setup(&root);

    let t = Instant::now();
    let (report, new_rel) =
        rename_with_rewrite(&root, "A.md", "B.md", &rels, &backups).expect("联合操作");
    let ms = t.elapsed().as_millis();
    println!("NFR-PERF-16（联合操作）实测：{ms} ms");
    assert_eq!(new_rel, "B.md");
    assert_eq!(report.files, 300);
    assert!(root.join("B.md").exists() && !root.join("A.md").exists());
    assert!(ms < 15_000, "联合操作不得超过 15s（实测 {ms} ms）");
}
