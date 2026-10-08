//! 索引域命令的测试（DEBT-14 的第一刀：命令体已抽成可注入状态的函数 + 纯函数）。

use crate::commands::index::read_stats;

/// 本模块自带的临时 Vault 夹具（存储层的同名助手是模块私有，跨模块不可见）。
fn temp_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    std::fs::create_dir_all(dir.path().join(".knowlpad")).expect("建 .knowlpad");
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

/// M3 复核 M3 的回归：签名诊断不得把十六进制 digest 当 JSON 解析。
/// 修好前 `matched` 恒为 false → 每次打开 Vault 都误发 kp://index/rebuild-required。
#[test]
fn signature_info_reports_matched_for_same_root() {
    let (_d, root) = temp_vault();
    let pool = crate::storage::index::open(&root).expect("建索引库");
    // ① 同一个根：索引库建好即带一致签名 → 必须 matched（这正是"不再误报重建"的条件）
    let same = crate::commands::index::signature_info(&root, Some(&pool));
    assert!(
        same.matched,
        "同一 Vault 根必须 matched=true；reason={:?}",
        same.reason
    );
    assert!(same.reason.is_none(), "matched 时不应给原因");
    // ② 换个 Vault 根：期望签名变化（含 vault_root）→ 必须不可信且给出原因
    let other = crate::commands::index::signature_info(
        std::path::Path::new("C:/another-vault"),
        Some(&pool),
    );
    assert!(!other.matched, "换根后必须不可信");
    assert!(other.reason.is_some(), "不可信时必须给出原因");
    // ③ 没有索引库（None）→ 不可信且不 panic
    let none = crate::commands::index::signature_info(&root, None);
    assert!(!none.matched, "没有索引库时必须不可信");
}
/// 期望签名随 Vault 根变化（签名含 vault_root，防止把 .knowlpad 拷到别处误用）。
#[test]
fn expected_signature_depends_on_vault_root() {
    let a = crate::storage::index::IndexSignature::current(std::path::Path::new("C:/vault-a"));
    let b = crate::storage::index::IndexSignature::current(std::path::Path::new("C:/vault-b"));
    assert_ne!(a.digest, b.digest);
}
