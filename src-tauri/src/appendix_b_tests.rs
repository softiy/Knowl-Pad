//! 附录 B 的**索引层**用例（B-19/B-20/B-22）与**清点测试**。
//!
//! 清点测试的由来：独立审查发现「准确率 19/19 = 100%」的分母取自夹具自身 —— 夹具少几条，
//! 准确率反而更高（分母自证）。这里对着 **PRD 附录 B 的表格**数满 25 条，缺一条就失败。

use std::fs;
use std::path::{Path, PathBuf};

use kp_domain::error::AppError;

use crate::index_engine::{full_index, scan_vault};
use crate::storage::index::open;
use crate::storage::pool::DbPool;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/syntax-compat")
}

fn write_note(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).expect("建目录");
    }
    fs::write(p, content).expect("写笔记");
}

/// 从 PRD 附录 B 的表格里抽出全部用例编号（真相源）。
fn prd_case_ids() -> Vec<String> {
    let prd = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/Knowl-Pad-PRD.md"),
    )
    .expect("应能读 PRD");
    let mut ids: Vec<String> = prd
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            let rest = t.strip_prefix("| ")?;
            let id = rest.split(' ').next()?;
            if id.len() == 4 && id.starts_with("B-") && id[2..].chars().all(|c| c.is_ascii_digit())
            {
                Some(id.to_string())
            } else {
                None
            }
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// 本测试文件覆盖的索引层用例（其余在 kp-domain 的夹具或顺延清单里）。
const INDEX_LAYER_CASES: [&str; 3] = ["B-19", "B-20", "B-22"];
/// 已正式顺延的用例（PRD 勘误登记，见 expected.json 的 deferred）。
const DEFERRED_CASES: [&str; 1] = ["B-25"];

/// **清点**：附录 B 的每一条都必须有着落（解析层夹具 / 索引层用例 / 顺延清单），且不重不漏。
#[test]
fn appendix_b_inventory_has_no_gap() {
    let ids = prd_case_ids();
    assert_eq!(
        ids.len(),
        25,
        "PRD 附录 B 应有 25 条，实际 {}：{:?}",
        ids.len(),
        ids
    );

    let exp: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fixture_dir().join("expected.json")).expect("应能读夹具"),
    )
    .expect("夹具应是合法 JSON");
    let mut covered: Vec<String> = exp["cases"]
        .as_array()
        .expect("cases 应是数组")
        .iter()
        .map(|c| c["id"].as_str().expect("id 应是字符串").to_string())
        .collect();
    covered.extend(INDEX_LAYER_CASES.iter().map(|s| s.to_string()));
    covered.extend(DEFERRED_CASES.iter().map(|s| s.to_string()));
    covered.sort();
    let mut deduped = covered.clone();
    deduped.dedup();
    assert_eq!(covered, deduped, "有用例被重复认领：{covered:?}");

    let missing: Vec<&String> = ids.iter().filter(|id| !covered.contains(id)).collect();
    let extra: Vec<&String> = covered.iter().filter(|id| !ids.contains(id)).collect();
    assert!(
        missing.is_empty(),
        "附录 B 里这些用例没有任何着落：{missing:?}"
    );
    assert!(extra.is_empty(), "这些用例不在附录 B 里：{extra:?}");
    assert_eq!(covered.len(), 25, "覆盖数应为 25");
}

/// B-19：不同文件夹下的同名笔记 → `ambiguous`（**不猜**）。
#[test]
fn b19_same_stem_in_two_folders_is_ambiguous() {
    let dir = tempfile::tempdir().expect("临时目录");
    write_note(dir.path(), "甲/note.md", "# 甲\n");
    write_note(dir.path(), "乙/note.md", "# 乙\n");
    write_note(dir.path(), "引用.md", "见 [[note]]\n");
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("索引");
    let status = link_status(&pool, "note");
    assert_eq!(
        status, "ambiguous",
        "同名笔记应判为歧义（候选不唯一时不猜）"
    );
    // 说明：PRD 要求「记录全部候选」，但 link 表当前没有候选列 —— 该缺口已登记在台账。
    let candidates: i64 = pool
        .with_reader(|conn| {
            conn.query_row("SELECT count(*) FROM file WHERE stem = 'note'", [], |r| {
                r.get(0)
            })
            .map_err(|_| AppError::db("统计候选"))
        })
        .expect("统计候选");
    assert_eq!(candidates, 2, "候选数可推导（记录候选列仍待补）");
}

/// B-20：`[[Note]]` 对 `note.md` → 大小写不敏感匹配成功。
#[test]
fn b20_case_insensitive_match_resolves() {
    let dir = tempfile::tempdir().expect("临时目录");
    write_note(dir.path(), "note.md", "# 小写文件名\n");
    write_note(dir.path(), "引用.md", "见 [[Note]]\n");
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("索引");
    assert_eq!(
        link_status(&pool, "Note"),
        "resolved",
        "大小写不同也应匹配成功"
    );
}

/// B-22：含 `.obsidian/` 的 Vault → 该目录被忽略且不索引，其余正确解析（FR-STORAGE-02/04）。
#[test]
fn b22_third_party_config_dir_is_ignored() {
    let dir = tempfile::tempdir().expect("临时目录");
    write_note(dir.path(), "正常.md", "# 正常\n\n[[另一篇]]\n");
    write_note(dir.path(), "另一篇.md", "# 另一篇\n");
    write_note(dir.path(), ".obsidian/config.md", "# 第三方配置\n");
    write_note(dir.path(), ".knowlpad/内部.md", "# 内部\n");
    let (entries, _) = scan_vault(dir.path()).expect("遍历");
    let paths: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
    assert!(
        !paths.iter().any(|p| p.contains(".obsidian")),
        "第三方配置目录不得进索引：{paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains(".knowlpad")),
        "内部目录不得进索引：{paths:?}"
    );
    assert_eq!(paths.len(), 2, "只应索引两篇正常笔记：{paths:?}");
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("索引");
    assert_eq!(
        link_status(&pool, "另一篇"),
        "resolved",
        "其余笔记应正确解析并裁决"
    );
}

fn link_status(pool: &DbPool, target: &str) -> String {
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT status FROM link WHERE target_ref = ?1",
            rusqlite::params![target],
            |r| r.get::<_, String>(0),
        )
        .map_err(|_| AppError::db("读裁决"))
    })
    .expect("读裁决")
}
