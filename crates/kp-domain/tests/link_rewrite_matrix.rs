//! M4「测试集先行」：链接改写矩阵夹具的**完整性与清点**测试。
//!
//! 这是 M4 的第一个交付物（PR-M4-1）：**先建立测试集**，再实现改写器。
//! 它不依赖任何尚未实现的 API —— 只校验"测试集本身"是否成立：
//!   ① 每个用例都含必需字段，id 唯一；
//!   ② **受保护上下文**（代码块 / 行内代码 / frontmatter / HTML 注释）都至少有一个"必须不动"的反例；
//!   ③ **双向清点**：PRD 里的全部链接相关编号都被 M4 计划覆盖；cases 的 covers 里每个编号都真实存在于 PRD。
//!      （M3 的教训：分母必须来自真相源，不能让夹具自证。）

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn load_cases() -> serde_json::Value {
    let path = repo_path("tests/fixtures/link-rewrite/cases.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读不到夹具 {}: {e}", path.display()));
    serde_json::from_str(&text).expect("cases.json 必须是合法 JSON")
}

#[test]
fn every_case_has_required_fields_and_unique_id() {
    let json = load_cases();
    let cases = json["cases"].as_array().expect("cases 必须是数组");
    assert!(
        cases.len() >= 15,
        "矩阵至少要 15 个用例（当前 {}）",
        cases.len()
    );
    let mut ids = BTreeSet::new();
    for c in cases {
        let id = c["id"].as_str().expect("每个用例要有 id");
        assert!(ids.insert(id.to_string()), "id 重复：{id}");
        for field in ["form", "context", "input", "expected", "covers"] {
            assert!(!c[field].is_null(), "用例 {id} 缺字段 {field}");
        }
        assert!(
            c["shouldRewrite"].is_boolean(),
            "用例 {id} 的 shouldRewrite 必须是布尔"
        );
        let covers = c["covers"].as_array().expect("covers 必须是数组");
        assert!(!covers.is_empty(), "用例 {id} 的 covers 不能为空");
        if c["shouldRewrite"].as_bool().unwrap() {
            assert_ne!(
                c["input"], c["expected"],
                "用例 {id} 声明应改写但输入输出相同"
            );
        } else {
            assert_eq!(
                c["input"], c["expected"],
                "用例 {id} 声明不改写但输入输出不同"
            );
        }
    }
}

/// 受保护上下文必须有反例 —— 否则"不改代码块"这类要求可能形同虚设。
#[test]
fn protected_contexts_all_have_counterexamples() {
    let json = load_cases();
    let cases = json["cases"].as_array().unwrap();
    for ctx in ["fenced-code", "inline-code", "frontmatter", "html-comment"] {
        let found = cases
            .iter()
            .any(|c| c["context"] == ctx && c["shouldRewrite"].as_bool() == Some(false));
        assert!(found, "受保护上下文 `{ctx}` 没有任何“必须不动”的反例");
    }
}

/// 双向清点（反"分母自证"）：
/// A. PRD 里的每个链接相关编号 → 必须出现在 M4 计划文档里（有归属）；
/// B. cases.json 的 covers 里的每个编号 → 必须真实存在于 PRD（防拼写/幻觉）。
#[test]
fn link_ids_are_bidirectionally_accounted_for() {
    let prd = std::fs::read_to_string(repo_path("docs/Knowl-Pad-PRD.md")).expect("读 PRD");
    let plan =
        std::fs::read_to_string(repo_path("docs/history/M4任务拆解与验收清单-2026-10-07.md"))
            .expect("读 M4 计划");
    let json = load_cases();

    // 从 PRD 抽编号（真相源里的分母）
    let mut prd_ids: BTreeSet<String> = BTreeSet::new();
    for (i, _) in prd.match_indices("FR-LINK-") {
        let tail = &prd[i..];
        let digits: String = tail["FR-LINK-".len()..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            prd_ids.insert(format!("FR-LINK-{digits}"));
        }
    }
    for id in [
        "FR-FILE-21",
        "FR-FILE-22",
        "AC-FILE-01",
        "AC-FILE-02",
        "NFR-PERF-16",
    ] {
        assert!(prd.contains(id), "PRD 里应存在 {id}（真相源校验失败）");
        prd_ids.insert(id.to_string());
    }
    for n in 1..=5 {
        let id = format!("AC-LINK-0{n}");
        assert!(prd.contains(&id), "PRD 里应存在 {id}");
        prd_ids.insert(id);
    }
    assert!(
        prd_ids.len() >= 20,
        "从 PRD 抽到的链接编号太少（{}），抽取逻辑可能失效",
        prd_ids.len()
    );

    // A. 每个 PRD 编号都要在 M4 计划里有归属
    let mut missing_in_plan = Vec::new();
    for id in &prd_ids {
        if !plan.contains(id.as_str()) {
            missing_in_plan.push(id.clone());
        }
    }
    assert!(
        missing_in_plan.is_empty(),
        "这些编号在 M4 计划里没有归属：{missing_in_plan:?}"
    );

    // B. 夹具声明的每个编号都要真实存在
    let cases = json["cases"].as_array().unwrap();
    let mut phantom = Vec::new();
    for c in cases {
        for cov in c["covers"].as_array().unwrap() {
            let id = cov.as_str().unwrap();
            if !prd_ids.contains(id) {
                phantom.push(format!("{} -> {id}", c["id"].as_str().unwrap()));
            }
        }
    }
    assert!(
        phantom.is_empty(),
        "夹具引用了 PRD 中不存在的编号：{phantom:?}"
    );
}

/// **测试集先行终于兑现**：让矩阵里的每一条用例直接驱动 `link_rewrite::rewrite_references`。
///
/// 这是 M4 最初那批交付（夹具 + 清点）的用途 —— 实现写完后，15 条用例（含 5 条"必须不动"）
/// 就是判据：任何一条红都说明改写器会损坏用户文件或漏改。
#[test]
fn matrix_cases_drive_the_rewriter() {
    let json = load_cases();
    let from = json["meta"]["rename"]["from"]
        .as_str()
        .expect("夹具要给出 rename.from");
    let to = json["meta"]["rename"]["to"]
        .as_str()
        .expect("夹具要给出 rename.to");
    let cases = json["cases"].as_array().expect("cases 必须是数组");
    let mut failures: Vec<String> = Vec::new();
    for c in cases {
        let id = c["id"].as_str().unwrap();
        let input = c["input"].as_str().unwrap();
        let expected = c["expected"].as_str().unwrap();
        let should = c["shouldRewrite"].as_bool().unwrap();
        let (got, changed) = kp_domain::link_rewrite::rewrite_references(input, from, to);
        if got != expected {
            failures.push(format!(
                "{id}: 期望 {expected:?}，实际 {got:?}（改写 {changed} 处）"
            ));
        }
        if should && changed == 0 {
            failures.push(format!("{id}: 声明应改写，但实际改写 0 处"));
        }
        if !should && changed != 0 {
            failures.push(format!(
                "{id}: 声明**不得**改写（受保护上下文），但实际改了 {changed} 处"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "改写矩阵有 {} 条不达标：\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
