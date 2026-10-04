//! 设置域命令的契约形状与参数解析测试（CODE-11：测试位于独立文件）。

use super::*;

#[test]
fn keys_args_accepts_missing_and_present() {
    let none: KeysArgs = serde_json::from_str("{}").expect("keys 应可缺省");
    assert!(none.keys.is_none(), "缺省表示读取全部");
    let some: KeysArgs = serde_json::from_str(r#"{"keys":["ui.showHiddenFiles"]}"#).unwrap();
    assert_eq!(some.keys.unwrap(), vec!["ui.showHiddenFiles".to_string()]);
}

#[test]
fn entries_args_parses_camel_case_json_values() {
    let args: EntriesArgs =
        serde_json::from_str(r#"{"entries":{"ui.showHiddenFiles":true,"tree.expanded":["a"]}}"#)
            .expect("应能解析 values");
    assert_eq!(
        args.entries.get("ui.showHiddenFiles"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        args.entries.get("tree.expanded"),
        Some(&serde_json::json!(["a"]))
    );
}

#[test]
fn values_result_serializes_values_map() {
    let mut values = BTreeMap::new();
    values.insert("k".to_string(), serde_json::json!({"nested": 1}));
    let json = serde_json::to_value(ValuesResult { values }).unwrap();
    assert_eq!(json["values"]["k"]["nested"], 1);
}

#[test]
fn parse_value_falls_back_to_string_for_legacy_plain_text() {
    // 旧版本可能写入过裸字符串（非 JSON）：读取时降级为字符串而不是报错
    assert_eq!(parse_value("dark"), serde_json::json!("dark"));
    assert_eq!(parse_value("\"dark\""), serde_json::json!("dark"));
    assert_eq!(parse_value("42"), serde_json::json!(42));
}
