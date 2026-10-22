//! **AC-REL-02 的回归用例**（2026-10-22，P0）：自动对账补齐 + 就绪信号 + `.old` 清理。
//!
//! 事实（技术方案 / PRD 口径）：打开 Vault 与启动恢复都会走 `reconcile_after_open` → `run_reconcile`
//! （`SkipUnchanged` 全库对账）⇒ 强杀后的半索引会被**自动补齐**。本文件钉住的是「机制真的被接在关键路径上」：
//! 标记从索引**开始前**就在（崩溃与取消共用同一套），重开判 `Rebuilt`，对账完成前不宣称就绪，`.old` 不残留。
//!
//! **模拟口径**：链式用例用「取消后不清理」来**等价模拟**强杀（标记已写、没有任何收尾代码执行）；
//! 真 kill 的库完整性由 `perf_d25_tests::d25_ac_rel_02_kill_mid_index` 覆盖 —— 两条用例分工不同，不要互相冒充。

use std::fs;
use std::path::PathBuf;

use crate::commands::vault::run_reconcile;
use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::state::AppState;
use crate::storage::index::{open, read_meta, IndexSignature, META_REBUILD_IN_PROGRESS};
use crate::storage::index_rebuild::{ensure_index_db, RebuildOutcome};
use crate::storage::pool::DbPool;

fn build_vault(n: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("临时 Vault");
    let root = dir.path().to_path_buf();
    for i in 0..n {
        let p = root.join(format!("d{}/note-{i}.md", i % 5));
        fs::create_dir_all(p.parent().expect("父目录")).expect("建目录");
        fs::write(&p, format!("# 笔记 {i}\n\n[[d0/note-0]] #关系/测试\n")).expect("写笔记");
    }
    (dir, root)
}

/// 返回（守卫, 池的 vault-root, index.db 路径）。`storage::index::open` 收的是 **vault root**（内部再拼 `.knowlpad/index.db`），
/// 而 `ensure_index_db` 收的是**库路径** —— 两个参数别混。
/// 库放在 Vault 内（与生产一致：`<vault>/.knowlpad/index.db`）—— 否则 `open()` 的签名 root 与
/// `ensure_index_db` 的 root 不同，会得到「vault_root 变化」的假失败。
fn vault_with_db(n: usize) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (guard, root) = build_vault(n);
    let db = root.join(".knowlpad").join("index.db");
    (guard, root, db)
}

/** 逐表计数（用于「与干净重建一致」的断言；不用 contains，避免假阳性）。 */
fn counts(pool: &DbPool) -> Vec<(String, i64)> {
    const TABLES: [&str; 6] = ["file", "link", "tag", "file_tag", "heading", "note_fts"];
    pool.with_reader(|c| {
        let mut out = Vec::new();
        for t in TABLES {
            let n: i64 = c
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .map_err(|e| kp_domain::error::AppError::db_with("逐表计数", &e))?;
            out.push((t.to_string(), n));
        }
        Ok(out)
    })
    .expect("逐表计数")
}

/// **链式**：标记在 ⇒ 重开判 Rebuilt ⇒ 打开后的对账**自动补齐** ⇒ 与干净重建逐表一致（不手工补跑）。
#[test]
fn interrupted_index_is_rebuilt_and_auto_reconciled() {
    let (vault_guard, root, db) = vault_with_db(1000);
    let pool_root = root.clone();
    let sig = IndexSignature::current(&root);
    assert!(matches!(
        ensure_index_db(&db, &sig).expect("建库"),
        RebuildOutcome::Fresh
    ));

    // 等价模拟强杀：取消在第 3 批之前（标记已写、没有任何收尾）—— 真 kill 由 d25_ac_rel_02_kill_mid_index 覆盖。
    {
        let pool = open(&pool_root).expect("打开");
        // 取消谓词是 `Fn`：用原子计数器（第一批之后取消）
        let seen = std::sync::atomic::AtomicUsize::new(0);
        let outcome = full_index_cancellable(
            &pool,
            &root,
            |_, _| {},
            IndexMode::Force,
            || seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1,
        )
        .expect("可取消索引");
        assert!(outcome.cancelled, "取消必须被观测到");
    }

    // 标记必须还在（这是「重开判 Rebuilt」的唯一依据）。
    // 注意：**不能**用 `open()` 来读 —— 它会先跑 ensure_index_db，见到标记就重建、标记随之消失。
    {
        let conn = rusqlite::Connection::open(&db).expect("只读连接");
        let marker = read_meta(&conn, META_REBUILD_IN_PROGRESS).expect("读标记");
        assert!(
            marker.is_some(),
            "取消/崩溃后必须留下 rebuild_in_progress 标记"
        );
    }

    // 重开 ⇒ Rebuilt（半索引不得被信任）
    let reopened = ensure_index_db(&db, &sig).expect("重开判定");
    assert!(
        matches!(reopened, RebuildOutcome::Rebuilt { .. }),
        "重开应判 Rebuilt，实得 {reopened:?}"
    );

    // 打开后的**自动对账**（生产路径 vault_open → reconcile_after_open → run_reconcile），**不手工补跑全量索引**
    let pool = open(&pool_root).expect("打开");
    let state = AppState::new();
    assert!(!state.index_ready(), "对账前不得宣称就绪");
    let outcome = run_reconcile(&state, &pool, &root, |_, _| {}).expect("自动对账");
    assert!(!outcome.cancelled);
    assert!(outcome.indexed > 0, "自动对账必须真的补进内容");
    assert!(state.index_ready(), "对账完成后才置就绪");

    // 与「从零干净重建」逐表一致
    let (clean_guard, clean_pool_root, clean_db) = vault_with_db(1000);
    ensure_index_db(&clean_db, &sig).expect("干净建库");
    let clean_pool = open(&clean_pool_root).expect("打开干净库");
    run_reconcile(&AppState::new(), &clean_pool, &root, |_, _| {}).expect("干净对账");
    let got = counts(&pool);
    let want = counts(&clean_pool);
    assert!(got[0].1 > 0, "file 表不应为空：{got:?}");
    assert_eq!(got, want, "自动补齐后的索引必须与干净重建逐表一致");
    {
        let conn = rusqlite::Connection::open(&db).expect("只读连接");
        let marker = read_meta(&conn, META_REBUILD_IN_PROGRESS).expect("读标记");
        assert!(marker.is_none(), "成功后标记必须清除");
    }
    let _ = (&vault_guard, &clean_guard);
}

/// 正常跑完 ⇒ 标记清除 ⇒ 重开判 `Unchanged`（不重建）。
#[test]
fn completed_index_clears_marker_and_matches_on_reopen() {
    let (vault_guard, root, db) = vault_with_db(30);
    let pool_root = root.clone();
    let sig = IndexSignature::current(&root);
    ensure_index_db(&db, &sig).expect("建库");
    let pool = open(&pool_root).expect("打开");
    let state = AppState::new();
    run_reconcile(&state, &pool, &root, |_, _| {}).expect("对账");
    assert!(state.index_ready());
    let marker = pool
        .with_reader(|c| read_meta(c, META_REBUILD_IN_PROGRESS))
        .expect("读标记");
    assert!(marker.is_none(), "成功完成后标记必须被清除");
    drop(pool);
    let again = ensure_index_db(&db, &sig).expect("重开判定");
    assert!(
        matches!(again, RebuildOutcome::Unchanged),
        "正常完成后重开应是 Unchanged，实得 {again:?}"
    );
    let _ = &vault_guard;
}

/// 对账失败 ⇒ 返回 Err 且**不宣称就绪**（调用方据此不启动 watcher，见 `spawn_reconcile_and_watch` 的 Err 分支）。
#[test]
fn reconcile_failure_keeps_not_ready() {
    let (_vault_guard, root, db) = vault_with_db(5);
    let pool_root = root.clone();
    let sig = IndexSignature::current(&root);
    ensure_index_db(&db, &sig).expect("建库");
    let pool = open(&pool_root).expect("打开");
    let state = AppState::new();
    // 用「一个普通文件」当根：扫描必然失败（不存在的目录在 scan_vault 里可能是空列表，不够硬）
    let missing = root.join("not-a-vault-dir.md");
    std::fs::write(&missing, b"# x\n").expect("造一个文件");
    let result = run_reconcile(&state, &pool, &missing, |_, _| {});
    assert!(result.is_err(), "对账路径不可用时应返回 Err");
    assert!(!state.index_ready(), "对账失败后不得宣称就绪");
    let _ = &root;
}

/// `.old` 残留：签名匹配（`Matches` 提前 return）的那条路径也要清掉它。
#[test]
fn stale_old_file_is_removed_on_open() {
    let (_vault_guard, root, db) = vault_with_db(5);
    let sig = IndexSignature::current(&root);
    ensure_index_db(&db, &sig).expect("建库");
    let old = db.with_file_name("index.db.old");
    fs::write(&old, b"stale").expect("造 .old");
    fs::write(old.with_file_name("index.db.old-wal"), b"stale").expect("造 -wal");
    let outcome = ensure_index_db(&db, &sig).expect("重开判定");
    assert!(matches!(outcome, RebuildOutcome::Unchanged));
    assert!(!old.exists(), ".old 必须被清理");
    assert!(
        !old.with_file_name("index.db.old-wal").exists(),
        ".old-wal 必须被清理"
    );
    let _ = &root;
}

/// **DEBT-45 的链式判定**（审查：此前只有分散的三处断言）。
///
/// 分工（与同文件第一条链式用例互补，不要互相冒充）：
/// - 本用例只覆盖「取消 ⇒ 标记 ⇒ **重开判定** = `Rebuilt`」（判定本身）；
/// - `interrupted_index_is_rebuilt_and_auto_reconciled` 覆盖「重开后**自动对账补齐** + 逐表一致」。
#[test]
fn cancelled_index_is_judged_rebuilt_on_reopen() {
    let (_vault_guard, root) = build_vault(400);
    let db = root.join(".knowlpad").join("index.db");
    let sig = IndexSignature::current(&root);
    assert!(matches!(
        ensure_index_db(&db, &sig).expect("建库"),
        RebuildOutcome::Fresh
    ));

    // 取消（第一次批间检查就停）⇒ 引擎已在**开始前**写了标记、且不会清
    {
        let pool = open(&root).expect("打开");
        let outcome = full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || true)
            .expect("可取消索引");
        assert!(outcome.cancelled);
    } // drop pool：模拟进程结束（没有任何收尾代码）

    // 链式：重开必须判 Rebuilt（而不是沿用半索引）
    let (_pool, outcome) = crate::storage::index::open_with_outcome(&root).expect("重开判定");
    assert!(
        matches!(outcome, RebuildOutcome::Rebuilt { .. }),
        "取消后重开应判 Rebuilt，实得 {outcome:?}"
    );
}
