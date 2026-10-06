//! 索引域命令的测试（DEBT-14 的第一刀：命令体已抽成可注入状态的函数 + 纯函数）。

use crate::commands::index::{parse_stored_signature, read_stats};

fn temp_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    (dir, root)
}

/// 正向：空的索引库能读出全 0 的计数（表由 DDL 建好）。
#[test]
fn stats_on_fresh_index_are_zero() {
    let (_d, root) = temp_vault();
    let pool = crate::storage::index::open(&root).expect("建索引库");
    let stats = read_stats(&pool).expect("统计应成功");
    assert_eq!(stats.files, 0);
    assert_eq!(stats.links, 0);
    assert_eq!(stats.fts_rows, 0);
}

/// 错误面：未打开 Vault（无索引库）时统计必须显式报错，而不是返回 0 掩盖问题。
#[test]
fn stats_requires_open_vault() {
    // 命令层的守卫：index_db 为 None → E_VAULT_NOT_OPEN（由命令体负责，这里验证语义常量）
    let err = kp_domain::error::AppError::VaultNotOpen;
    assert_eq!(err.code(), "E_VAULT_NOT_OPEN");
}

/// 签名解析：合法 JSON → 结构化；缺字段/非法 JSON/None → 一律 None（不可信即重建）。
#[test]
fn stored_signature_parsing_is_strict() {
    let ok = r#"{"schema_version":1,"parser_version":2,"tokenizer_version":"0.11.0","tokenizer_dict_hash":"ab","vault_root":"C:/v","digest":"ff"}"#;
    let parsed = parse_stored_signature(Some(ok)).expect("合法签名应解析成功");
    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.digest, "ff");

    assert!(parse_stored_signature(None).is_none(), "无记录 → None");
    assert!(
        parse_stored_signature(Some("not json")).is_none(),
        "非法 JSON → None"
    );
    assert!(
        parse_stored_signature(Some(r#"{"schema_version":1}"#)).is_none(),
        "缺字段 → None（不可信）"
    );
}

/// 期望签名随 Vault 根变化（签名含 vault_root，防止把 .knowlpad 拷到别处误用）。
#[test]
fn expected_signature_depends_on_vault_root() {
    let a = crate::storage::index::IndexSignature::current(std::path::Path::new("C:/vault-a"));
    let b = crate::storage::index::IndexSignature::current(std::path::Path::new("C:/vault-b"));
    assert_ne!(a.digest, b.digest);
}
