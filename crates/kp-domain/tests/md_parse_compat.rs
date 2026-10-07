//! 附录 B 解析器兼容性用例（数据驱动：解析器输出 vs 预固化夹具）。
//!
//! 固化流程见 PRD 附录 B：期望值由 §3.1 条款推导、以结构化夹具入库，**CI 不引入任何外部软件依赖**。
//! 本文件覆盖 M3 PR-2 第一步的切片（code_fence + wikilink，10/25 条用例）。

use std::fs;
use std::path::PathBuf;

use kp_domain::md_parse::{parse, LineEnding, LinkKind};

#[derive(serde::Deserialize)]
struct Expected {
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    file: String,
    rule: String,
    links: Vec<ExpectedLink>,
    #[serde(default)]
    has_bom: Option<bool>,
    #[serde(default)]
    line_ending: Option<String>,
    #[serde(default)]
    fm_tags: Option<Vec<String>>,
    #[serde(default)]
    fm_aliases: Option<Vec<String>>,
    #[serde(default)]
    fm_invalid: Option<bool>,
}

#[derive(serde::Deserialize)]
struct ExpectedLink {
    target: String,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    alias: Option<String>,
    kind: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/syntax-compat")
}

#[test]
fn appendix_b_slice_accuracy_is_100_percent() {
    let dir = fixtures_dir();
    let raw = fs::read_to_string(dir.join("expected.json")).expect("夹具 expected.json 应存在");
    let expected: Expected = serde_json::from_str(&raw).expect("夹具 JSON 应可解析");
    let total = expected.cases.len();
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for case in &expected.cases {
        let bytes = fs::read(dir.join(&case.file))
            .unwrap_or_else(|e| panic!("{} 读取失败: {e}", case.file));
        let note = parse(&bytes);
        let mut ok = note.links.len() == case.links.len();
        if ok {
            for (got, want) in note.links.iter().zip(case.links.iter()) {
                let kind_ok = match want.kind.as_str() {
                    "embed" => got.kind == LinkKind::Embed,
                    _ => got.kind == LinkKind::Link,
                };
                if got.target != want.target
                    || got.anchor.as_deref() != want.anchor.as_deref()
                    || got.alias.as_deref() != want.alias.as_deref()
                    || !kind_ok
                {
                    ok = false;
                    break;
                }
            }
        }
        if let Some(bom) = case.has_bom {
            ok = ok && note.has_bom == bom;
        }
        if let Some(tags) = case.fm_tags.as_ref() {
            let got = note
                .frontmatter
                .as_ref()
                .map(|f| f.tags.clone())
                .unwrap_or_default();
            ok = ok && &got == tags;
        }
        if let Some(aliases) = case.fm_aliases.as_ref() {
            let got = note
                .frontmatter
                .as_ref()
                .map(|f| f.aliases.clone())
                .unwrap_or_default();
            ok = ok && &got == aliases;
        }
        if case.fm_invalid == Some(true) {
            ok = ok
                && note.frontmatter.is_none()
                && note.warnings.iter().any(|w| w.contains("FRONTMATTER"));
        }
        if let Some(le) = case.line_ending.as_deref() {
            let want = if le == "Crlf" {
                LineEnding::Crlf
            } else {
                LineEnding::Lf
            };
            ok = ok && note.line_ending == want;
        }
        if ok {
            passed += 1;
        } else {
            failures.push(format!("{}（{}）实际 {:?}", case.id, case.rule, note.links));
        }
    }

    let rate = passed as f64 / total as f64 * 100.0;
    println!("附录 B 切片准确率：{passed}/{total} = {rate:.1}%");
    assert!(failures.is_empty(), "未通过用例：\n{}", failures.join("\n"));
    assert!(rate >= 99.0, "准确率 {rate:.1}% 低于 99%");
}
