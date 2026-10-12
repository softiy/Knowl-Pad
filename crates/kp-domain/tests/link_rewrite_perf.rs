//! NFR-PERF-16：**重命名 + 链接改写（300 处引用）< 3s / 5s / 15s**。
//!
//! **本文件的墙钟数字只打印、不断言**（2026-10-12 修正）。
//!
//! 原本这里还断言了"总耗时 < 15s"，理由是"给宽松上限就不会 flaky" —— 实测被证伪：
//! Windows CI runner 在并发负载下写 300 个文件（每个都是 临时文件+rename+fsync）会超过 15s，
//! 于是 Windows 冒烟 job 随机变红（#112 就跑出过 `link_rewrite_perf.rs:50` 与 `:80` 两处失败），
//! 而同一提交在别的 runner 上是绿的 —— 这正是"用墙钟断言做回归"的经典失效模式。
//!
//! 与 M4 计划 §8 的原意一致：单元测试只负责**正确性**（300 个文件都命中、都真的被改写、改名到位），
//! 墙钟数字打印出来供人看日志。
//!
//! **注意（不许含糊）**：墙钟回归目前**还没有**任何自动门禁在管 —— 门禁 10b
//! （scripts/run-index-perf.mjs + perf/ 基线）只覆盖**索引侧**，链接改写侧尚未纳入。
//! 这一点登记在 docs/history/M1-M4复核台账-2026-10-11.md 的 T-30 条目里，属未完成项。
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
    // 刻意**不**断言墙钟：见文件头。正确性由下面两条与 hits 断言保证。
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
    // 同样刻意不断言墙钟（见文件头）；正确性由 new_rel / files / 改名结果保证。
}
