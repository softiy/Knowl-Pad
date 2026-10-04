//! 文件域写命令的契约形状测试（CODE-11：测试位于独立文件）。

use super::*;

fn to_json<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("序列化应成功")
}

#[test]
fn rename_result_matches_prd_shape() {
    let json = to_json(&RenameResult {
        from: "a.md".into(),
        to: "b.md".into(),
        new_mtime_ms: 42,
    });
    assert_eq!(json["from"], "a.md");
    assert_eq!(json["to"], "b.md");
    assert_eq!(json["newMtimeMs"], 42);
    assert!(
        json.get("backedUpTo").is_none(),
        "契约只暴露 from/to/newMtimeMs"
    );
}

#[test]
fn delete_result_matches_prd_shape() {
    let json = to_json(&DeleteResult {
        rel_path: "notes/a.md".into(),
        trashed_count: 3,
    });
    assert_eq!(json["relPath"], "notes/a.md");
    assert_eq!(json["trashedCount"], 3);
    assert!(json.get("trashRelPath").is_none(), "回收站内部路径不进契约");
}

#[test]
fn write_result_keeps_existing_contract() {
    let json = to_json(&WriteResult {
        rel_path: "a.md".into(),
        new_mtime: 7,
    });
    assert_eq!(json["relPath"], "a.md");
    assert_eq!(
        json["newMtime"], 7,
        "note_create 与 note_write 共用 WriteResult"
    );
}

#[test]
fn on_conflict_defaults_to_cancel() {
    // R-07：未显式指定时一律最安全（取消），绝不静默覆盖
    assert_eq!(policy_of(None).expect("缺省应可用"), ConflictPolicy::Cancel);
    assert_eq!(
        policy_of(Some("overwrite".into())).unwrap(),
        ConflictPolicy::Overwrite
    );
    assert_eq!(
        policy_of(Some("renameNew".into())).unwrap(),
        ConflictPolicy::RenameNew
    );
    assert_eq!(
        policy_of(Some("cancel".into())).unwrap(),
        ConflictPolicy::Cancel
    );
    let err = policy_of(Some("bogus".into())).expect_err("未知策略应被拒绝");
    assert!(err.to_string().contains("未知的冲突处理方式"));
}

#[test]
fn args_deserialize_from_camel_case_ipc_payloads() {
    let create: NoteCreateArgs =
        serde_json::from_str(r##"{"relPath":"a.md","content":"# A","onConflict":"cancel"}"##)
            .expect("应能反序列化 note_create 入参");
    assert_eq!(create.rel_path, "a.md");
    assert_eq!(create.on_conflict.as_deref(), Some("cancel"));

    let delete: FileDeleteArgs =
        serde_json::from_str(r#"{"relPath":"dir"}"#).expect("recursive 应可缺省");
    assert!(!delete.recursive, "recursive 缺省必须是 false（安全默认）");

    let rename: FileRenameArgs =
        serde_json::from_str(r#"{"from":"a.md","to":"b.md"}"#).expect("onConflict 应可缺省");
    assert!(rename.on_conflict.is_none());
}
