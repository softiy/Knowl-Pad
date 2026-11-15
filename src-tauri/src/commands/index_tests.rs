//! 索引域命令的测试（DEBT-14 的第一刀：命令体已抽成可注入状态的函数 + 纯函数）。

use crate::commands::index::read_stats;
use crate::commands::vault::index_status_impl;
use crate::state::AppState;

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
///
/// 审计 T-03：此前这条用例**自己构造** `AppError::VaultNotOpen` 再断言错误码 ——
/// 那验证的是错误码表，不是命令的守卫；名字与行为不符（"什么都没验证"）。现在真的调用命令体。
#[test]
fn stats_requires_open_vault() {
    let state = crate::state::AppState::new();
    // 注意：IndexStats 没实现 Debug，所以不能用 expect_err（那要求 T: Debug）——用 match 更直白。
    let err = match crate::commands::index::index_stats_impl(&state) {
        Err(err) => err,
        Ok(stats) => panic!("没有索引库时必须报错，却返回了 files={}", stats.files),
    };
    assert_eq!(
        err.0.code(),
        "E_VAULT_NOT_OPEN",
        "必须显式报「未打开知识库」，而不是返回 0 掩盖问题"
    );
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

/// **T1 / SEC-03 / 勘误 D-46 的契约面**：最近一次索引的警告必须经 IndexStatus 透出。
///
/// 修好前的通道是**零**：启动对账把 outcome.warnings 丢了，而事件载荷按 PRD §5.4 不得扩展
/// （EVT-03）⇒ 用户看到「索引就绪」却不知道有文件被跳过。
#[test]
fn index_status_exposes_recent_index_warnings() {
    let (_d, root) = temp_vault();
    let pool = crate::storage::index::open(&root).expect("建索引库");
    let state = AppState::new();
    state.set_root(Some(root.clone()));
    state.set_index_db(Some(std::sync::Arc::new(pool)));

    let fresh = index_status_impl(&state).expect("索引状态");
    assert!(
        fresh.warnings.is_empty(),
        "尚未跑过索引时必须是空清单（不是缺字段/None）：{:?}",
        fresh.warnings
    );

    state.set_index_warnings(vec!["skip-timeout:超长单行.md".to_string()]);
    let after = index_status_impl(&state).expect("索引状态");
    assert_eq!(
        after.warnings,
        vec!["skip-timeout:超长单行.md".to_string()],
        "IndexStatus 必须原样带出最近一次索引的警告"
    );
}

/// **T1 根因的端到端回归**：启动对账必须把 outcome.warnings 落进状态，并**替换**上一个 Vault 的旧警告。
///
/// 用扫描侧的 `skip-oversize:` 制造确定性的真实警告（与 `skip-timeout:` 同一套前缀口径；
/// 等 10 秒的解析超时不适合放进单测，超时分支由 `index_engine_tests` 的零超时用例钉住）。
#[test]
fn reconcile_publishes_warnings_and_discards_previous_vault_ones() {
    let (_d, root) = temp_vault();
    let big = root.join("超限笔记.md");
    let size = (crate::index_scan::MAX_NOTE_BYTES as usize) + 1;
    std::fs::write(&big, vec![b'x'; size]).expect("写超限笔记");
    let pool = crate::storage::index::open(&root).expect("建索引库");
    let state = AppState::new();
    state.set_index_warnings(vec!["skip-timeout:上一个 Vault 的旧警告.md".to_string()]);

    let outcome =
        crate::commands::vault::run_reconcile(&state, &pool, &root, |_, _| {}).expect("对账应成功");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|w| w.starts_with("skip-oversize:")),
        "扫描必须记下超限跳过：{:?}",
        outcome.warnings
    );
    assert_eq!(
        state.index_warnings(),
        outcome.warnings,
        "状态里应是本轮警告"
    );
    assert!(
        !state.index_warnings().iter().any(|w| w.contains("旧警告")),
        "打开新 Vault 必须清空上一个 Vault 的警告：{:?}",
        state.index_warnings()
    );

    // 透出链路端到端：同一个 state 上 index_status 能看到被跳过的文件名
    state.set_root(Some(root.clone()));
    state.set_index_db(Some(std::sync::Arc::new(pool)));
    let status = index_status_impl(&state).expect("索引状态");
    assert!(status.ready, "对账成功后应就绪");
    assert!(
        status.warnings.iter().any(|w| w.contains("超限笔记.md")),
        "IndexStatus.warnings 必须带出被跳过的文件名：{:?}",
        status.warnings
    );
}

/// 干净的一轮对账（没有文件被跳过）必须把旧警告清成空 —— 否则「⚠ N 条」会永久挂在已修好的库上。
#[test]
fn clean_reconcile_run_clears_previous_warnings() {
    let (_d, root) = temp_vault();
    std::fs::write(root.join("普通.md"), b"# hi\n").expect("写笔记");
    let pool = crate::storage::index::open(&root).expect("建索引库");
    let state = AppState::new();
    state.set_index_warnings(vec!["skip-timeout:旧.md".to_string()]);

    let outcome =
        crate::commands::vault::run_reconcile(&state, &pool, &root, |_, _| {}).expect("对账应成功");
    assert!(
        outcome.warnings.is_empty(),
        "干净库不该有警告：{:?}",
        outcome.warnings
    );
    assert!(state.index_warnings().is_empty(), "旧警告必须被清空");
}

/// 期望签名随 Vault 根变化（签名含 vault_root，防止把 .knowlpad 拷到别处误用）。
#[test]
fn expected_signature_depends_on_vault_root() {
    let a = crate::storage::index::IndexSignature::current(std::path::Path::new("C:/vault-a"));
    let b = crate::storage::index::IndexSignature::current(std::path::Path::new("C:/vault-b"));
    assert_ne!(a.digest, b.digest);
}
