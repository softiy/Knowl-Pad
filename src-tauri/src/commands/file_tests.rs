//! 文件域命令的契约形状测试（CODE-11：测试位于独立文件）。

use super::*;
use kp_domain::file_tree::{FileEntry, FileKind};

fn to_json<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("序列化应成功")
}

#[test]
fn file_node_serializes_camel_case_and_omits_absent_options() {
    let node = FileNode::from(FileEntry {
        rel_path: "notes/a.md".into(),
        name: "a.md".into(),
        is_dir: false,
        kind: FileKind::Note,
        size_bytes: Some(3),
        mtime_ms: Some(1000),
        has_children: None,
    });
    let json = to_json(&node);
    assert_eq!(json["relPath"], "notes/a.md");
    assert_eq!(json["isDir"], false);
    assert_eq!(json["kind"], "note");
    assert_eq!(json["sizeBytes"], 3);
    assert_eq!(json["mtimeMs"], 1000);
    assert!(
        json.get("hasChildren").is_none(),
        "文件不应带 hasChildren 字段"
    );
}

#[test]
fn directory_node_keeps_has_children_and_omits_size() {
    let node = FileNode::from(FileEntry {
        rel_path: "notes".into(),
        name: "notes".into(),
        is_dir: true,
        kind: FileKind::Other,
        size_bytes: None,
        mtime_ms: Some(1),
        has_children: Some(true),
    });
    let json = to_json(&node);
    assert_eq!(json["hasChildren"], true);
    assert!(json.get("sizeBytes").is_none(), "目录不应带 sizeBytes 字段");
}

#[test]
fn validation_result_omits_reason_when_valid() {
    let ok = to_json(&ValidationResult {
        valid: true,
        reason: None,
    });
    assert_eq!(ok["valid"], true);
    assert!(ok.get("reason").is_none(), "合法时不应带 reason 字段");
    let bad = to_json(&ValidationResult {
        valid: false,
        reason: Some("含非法字符".into()),
    });
    assert_eq!(bad["valid"], false);
    assert_eq!(bad["reason"], "含非法字符");
}

#[test]
fn file_stat_serializes_camel_case() {
    let json = to_json(&FileStat {
        rel_path: "a.md".into(),
        is_dir: false,
        kind: "note".into(),
        size_bytes: 12,
        mtime_ms: 34,
    });
    for key in ["relPath", "isDir", "kind", "sizeBytes", "mtimeMs"] {
        assert!(json.get(key).is_some(), "缺少契约字段 {key}");
    }
}

#[test]
fn file_validate_name_maps_domain_result() {
    // 命令体极薄，此处断言域结果经契约映射后的语义（AC-FILE-03 的 5 个载荷）
    for payload in ["CON", "a/b", "名称.", "na*me", "  空格开头"] {
        let r = kp_domain::file_tree::validate_name(payload);
        assert!(!r.valid, "{payload} 应被拒绝");
        assert!(r.reason.is_some());
    }
    assert!(kp_domain::file_tree::validate_name("ok.md").valid);
}
