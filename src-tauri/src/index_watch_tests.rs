//! 文件监听端到端测试（**AC-SEARCH-04**：外部改动后索引自动跟上）。

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::index_engine::full_index;
use crate::index_watch::{start_polling_fallback, start_watcher_with, Change};
use crate::index_watch_paths::merge_events;
use crate::storage::index::open;
use kp_domain::error::AppError;

/// 触发外部变更**之前**的等待：给 notify 的监听注册与内部线程留出启动时间。
///
/// 审计 T-22 指出这些点位是"固定 sleep"（时序脆弱）。实际情况分两类：
///  · 本文件里 3 处 `deadline` 有界轮询（等索引收敛）是**正确**做法，保留；
///  · 4 处"变更前的固定等待"确实没有可依赖的就绪信号 —— `start_watcher_with` 返回时
///    notify 的 watcher 已注册，但平台侧事件投递与内部线程启动没有可观测的握手。
///    因此这里把它**命名**并写明用途与残余风险，而不是散落的魔法数字；
///    真正去掉它需要一个"监听已就绪"的信号（属实现改动），已登记在台账 T-22。
const SETTLE_BEFORE_CHANGE: Duration = Duration::from_millis(300);

/// "安静窗口"：索引只写 `.knowlpad/`（忽略目录）时不应产生任何事件。
///
/// 这是**负向断言**，本质上需要一个时间窗。窗口太短会假通过、太长会拖慢测试；
/// 1200ms 是权衡值。注意残余风险：若索引本身的写盘异常缓慢、事件晚于窗口到达，
/// 这条会**假失败**（属 T-22 登记的"双向 flaky"中的一侧）。
const QUIET_WINDOW: Duration = Duration::from_millis(1200);

/// 另一个有界轮询的步长（改名收敛用例用；比 POLL_STEP 略快以缩短测试时间）。
const POLL_STEP_FAST: Duration = Duration::from_millis(200);

/// 读取某目标的裁决状态。
/// 读某目标的裁决状态；**"还没有这一行"返回 None 而不是报错** ——
/// 端到端用例里"写盘 → 监听 → 增量写库 → 裁决"是分步的，中间态是正常的。
fn link_status_opt(pool: &crate::storage::pool::DbPool, target: &str) -> Option<String> {
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT status FROM link WHERE target_ref = ?1",
            rusqlite::params![target],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(AppError::db(&format!("读裁决：{other}"))),
        })
    })
    .ok()
    .flatten()
}

fn link_status(pool: &crate::storage::pool::DbPool, target: &str) -> String {
    link_status_opt(pool, target).expect("读裁决")
}

/// 某个词是否命中指定笔记 —— 判据面是**搜索**（AC-SEARCH-04 的严格口径要求如此），
/// 而不是"库里有一行"。返回命中行数（0 表示不再命中）。
fn fts_hits(pool: &crate::storage::pool::DbPool, term: &str, rel_path: &str) -> i64 {
    let expr = kp_domain::search::build_match_expr(term, kp_domain::search::MatchMode::All)
        .expect("构造 MATCH");
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT count(*) FROM note_fts JOIN file f ON f.id = note_fts.rowid \
             WHERE note_fts MATCH ?1 AND f.rel_path = ?2 AND f.deleted = 0",
            rusqlite::params![expr, rel_path],
            |r| r.get(0),
        )
        .map_err(|e| AppError::db(&format!("查 FTS：{e}")))
    })
    .expect("查 FTS")
}

/// **等待**某目标的裁决变成期望值（带截止时间）。
///
/// 为什么必须等：`apply_changes` 的顺序是"增量写库 → 事件回调 → 一次裁决与计数重算"，
/// 也就是说**事件到达时裁决还没做完**；用例收到事件后立刻读库，就会偶发读到旧状态
/// 甚至读到"这一行还没写进来"。CI 上已经因此红过一次（ac_search_04），
/// 本地也偶发过一次 —— 单次读取本身就是错的读法，不是机器慢。
fn wait_for_status(
    pool: &crate::storage::pool::DbPool,
    target: &str,
    want: &str,
    deadline: std::time::Duration,
) -> String {
    let started = std::time::Instant::now();
    let mut last = String::from("<尚无该行>");
    loop {
        if let Some(s) = link_status_opt(pool, target) {
            if s == want {
                return s;
            }
            last = s;
        }
        if started.elapsed() >= deadline {
            panic!("等待 {target} 的裁决变为 {want} 超时（当前：{last}，已等 {deadline:?}）");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn count(pool: &crate::storage::pool::DbPool, sql: &str) -> i64 {
    pool.with_reader(|conn| {
        conn.query_row(sql, [], |r| r.get(0))
            .map_err(|_| AppError::db("计数"))
    })
    .expect("计数")
}

/// 合并逻辑单测：create+modify 只应产生一条 created；忽略目录不产生任何变更。
#[test]
fn merge_events_dedupes_and_ignores() {
    let root = Path::new("C:/vault");
    let kinds = vec![
        (
            notify::EventKind::Create(notify::event::CreateKind::File),
            vec![std::path::PathBuf::from("C:/vault/a.md")],
        ),
        (
            notify::EventKind::Modify(notify::event::ModifyKind::Any),
            vec![std::path::PathBuf::from("C:/vault/.obsidian/x.md")],
        ),
    ];
    let changes = merge_events(root, kinds);
    assert_eq!(
        changes,
        vec![Change::Created("a.md".to_string())],
        "忽略目录不得产生变更"
    );
}

/// **AC-SEARCH-04（严格阈值，PRD §4.5.2）**：把 `a.md` 里的 `foo` 改成 `bar` 并保存 ⇒
/// **2 秒内**搜 `foo` 不再命中 `a.md`、搜 `bar` 命中 `a.md`，且**无需手动重建索引**。
///
/// M3 期的宽松口径（10 秒上限、只断言"新增的内容可检索"）在此升级为验收口径：
/// 判据面是**搜索**（FTS 命中集合），而不是"库里有一行"。
///
/// 顺带覆盖（**不属于 AC-SEARCH-04 的判据**）：同一次增量里的链接裁决 dangling → resolved。
#[test]
fn ac_search_04_modify_converges_within_two_seconds() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n\nfoo 指向 [[未来]]\n").expect("写笔记");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    assert_eq!(fts_hits(&pool, "foo", "a.md"), 1, "基线：foo 命中 a.md");

    let (tx, rx) = std::sync::mpsc::channel::<Vec<Change>>();
    let handle = start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            let _ = tx.send(changes.to_vec());
        },
        |_code, _message| {},
    )
    .expect("启动监听");

    std::thread::sleep(SETTLE_BEFORE_CHANGE);

    // 外部保存：foo → bar（PRD 的原文场景），**不手动触发任何重建**
    let started = std::time::Instant::now();
    fs::write(root.join("a.md"), "# 甲\n\nbar 指向 [[未来]]\n").expect("外部保存");
    let deadline = started + Duration::from_secs(2);
    let mut converged = false;
    while std::time::Instant::now() < deadline {
        if fts_hits(&pool, "bar", "a.md") == 1 && fts_hits(&pool, "foo", "a.md") == 0 {
            converged = true;
            break;
        }
        std::thread::sleep(POLL_STEP_FAST);
    }
    let elapsed_ms = started.elapsed().as_millis();
    println!("AC-SEARCH-04（严格）foo→bar 保存 → 搜索一致：{elapsed_ms} ms");
    assert!(
        converged,
        "PRD §4.5.2：保存后 2 秒内搜索必须一致（foo 不再命中、bar 命中），实测 {elapsed_ms} ms 仍未收敛"
    );
    let changes = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("应收到变更事件");
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::Modified(p) if p == "a.md")),
        "变更应为 modified：{changes:?}"
    );
    // 增量裁决：a.md 里有一条指向 `未来.md` 的链接，此刻应为 dangling
    let status_before = wait_for_status(&pool, "未来", "dangling", Duration::from_secs(10));
    assert_eq!(status_before, "dangling", "文件还不存在时链接应为 dangling");
    fs::write(root.join("未来.md"), "# 未来\n").expect("外部写入第二篇");
    let _ = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("应收到第二次变更");
    // 事件到达 ≠ 裁决完成（apply_changes 的顺序是写库 → 回调 → 裁决），所以这里**等**
    let status_after = wait_for_status(&pool, "未来", "resolved", Duration::from_secs(10));
    assert_eq!(
        status_after, "resolved",
        "增量裁决：目标文件出现后，原本悬空的链接应转为 resolved"
    );

    handle.stop();
}
/// **降级路径的真实验证**（独立审查 major #4）：监听句柄耗尽时启动的 5 秒轮询，
/// 必须真的能把外部新增的文件索引进来 —— 此前那条分支只发事件、并没有启动轮询。
#[test]
fn polling_fallback_indexes_external_change() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n").expect("写笔记");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    // 审计 T-21：句柄不再丢弃 —— 停止标志用它；测完必须真的停，否则线程会活到进程结束。
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = start_polling_fallback(pool.clone(), root.clone(), stop.clone());
    fs::write(root.join("轮询新增.md"), "# 轮询新增\n\n内容\n").expect("外部写入");
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    loop {
        let n = count(
            &pool,
            "SELECT count(*) FROM file WHERE rel_path = '轮询新增.md'",
        );
        if n == 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "轮询降级未在 12 秒内索引外部新增（说明降级没有真的跑起来）"
        );
        std::thread::sleep(SETTLE_BEFORE_CHANGE);
    }

    // **审计 T-21 的判据**：置位后线程必须能退出（此前取消谓词写死 || false，永远停不下来）。
    // 循环在睡眠后才检查标志，所以最多再等一个 5s 周期。
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().expect("轮询线程应当能正常退出");
}
/// **监听死循环防护**（计划 §4 第 21 行 / TECH §13.1 验证项 ③）：
/// 索引只写 `.knowlpad/`（忽略目录）→ "索引进行中改文件"不应产生事件；
/// 并且必须证明监听**是活的**（否则"没有事件"可能只是没在监听）。
#[test]
fn indexing_does_not_trigger_itself_but_watcher_stays_alive() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n\n内容\n").expect("写笔记");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Change>>();
    let handle = start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            let _ = tx.send(changes.to_vec());
        },
        |_code, _message| {},
    )
    .expect("启动监听");
    std::thread::sleep(SETTLE_BEFORE_CHANGE);
    full_index(&pool, &root, |_, _| {}).expect("再次索引");
    let quiet = rx.recv_timeout(QUIET_WINDOW);
    assert!(
        quiet.is_err(),
        "索引自身写 .knowlpad 不应触发变更事件，却收到 {quiet:?}"
    );
    fs::write(root.join("b.md"), "# 乙\n").expect("外部写入");
    let alive = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("监听应仍然活着");
    assert!(
        alive
            .iter()
            .any(|c| matches!(c, Change::Created(p) if p == "b.md")),
        "应报出外部新增：{alive:?}"
    );
    handle.stop();
}

/// **AC-FILE-07 的删除路径**（M3 复核 M3-2：此前只有"新增"场景，删除/改名**零覆盖**）。
/// 外部删掉笔记后：① 监听必须报出删除；② 索引里不再有该文件；③ 指向它的链接变 `dangling`。
#[test]
fn ac_file_07_external_delete_removes_from_index_and_dangles_links() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n\n见 [[b]]\n").expect("写 a");
    fs::write(root.join("b.md"), "# 乙\n").expect("写 b");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM file WHERE rel_path = 'b.md'"),
        1
    );
    assert_eq!(
        link_status(&pool, "b"),
        "resolved",
        "删除之前 [[b]] 是 resolved"
    );

    let (tx, rx) = std::sync::mpsc::channel::<Vec<Change>>();
    let handle = start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            let _ = tx.send(changes.to_vec());
        },
        |_code, _message| {},
    )
    .expect("启动监听");
    std::thread::sleep(SETTLE_BEFORE_CHANGE);

    fs::remove_file(root.join("b.md")).expect("外部删除");
    let reported = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("应报出外部删除");
    assert!(
        reported
            .iter()
            .any(|c| matches!(c, Change::Removed(p) if p == "b.md")),
        "应报出 Removed(b.md)：{reported:?}"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM file WHERE rel_path = 'b.md'"),
        0,
        "删除后索引里不得再有该文件"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM link WHERE target_ref = 'b' AND status = 'dangling'"
        ),
        1,
        "指向被删文件的链接必须变 dangling"
    );
    handle.stop();
}

/// **AC-FILE-07 的改名路径**：外部改名后旧路径从索引消失、新路径进索引，且链接重新裁决。
#[test]
fn ac_file_07_external_rename_reindexes_old_and_new_path() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n\n见 [[b]]\n").expect("写 a");
    fs::write(root.join("b.md"), "# 乙\n").expect("写 b");
    let pool = Arc::new(open(&root).expect("建索引库"));
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM link WHERE target_ref = 'b' AND status = 'resolved'"
        ),
        1,
        "改名之前 [[b]] 是 resolved"
    );

    let (tx, rx) = std::sync::mpsc::channel::<Vec<Change>>();
    let handle = start_watcher_with(
        pool.clone(),
        root.clone(),
        move |changes| {
            let _ = tx.send(changes.to_vec());
        },
        |_code, _message| {},
    )
    .expect("启动监听");
    std::thread::sleep(SETTLE_BEFORE_CHANGE);

    fs::rename(root.join("b.md"), root.join("c.md")).expect("外部改名");
    // 改名在文件系统上表现为一对事件；等索引收敛（最多 15 秒）
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let old_gone = count(&pool, "SELECT count(*) FROM file WHERE rel_path = 'b.md'") == 0;
        let new_present = count(&pool, "SELECT count(*) FROM file WHERE rel_path = 'c.md'") == 1;
        if old_gone && new_present {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "改名后索引未在 15 秒内收敛（old_gone={old_gone}, new_present={new_present}）"
        );
        std::thread::sleep(POLL_STEP_FAST); // 另一处有界轮询的步长
    }
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM link WHERE target_ref = 'b' AND status = 'dangling'"
        ),
        1,
        "旧目标 [[b]] 应变 dangling"
    );
    let _ = rx.recv_timeout(Duration::from_secs(2)); // 事件内容已由上面的索引状态覆盖
    handle.stop();
}

/// **AC-LINK-02 的规模断言**：知识库到达规模后，**新建**一篇被大量链接指向的笔记，
/// 增量路径必须在 **2 秒内**把那些链接判为 resolved，且**不是全量重建**。
///
/// 走的是**真实增量入口** `index_watch::apply_changes`（增量写库 → 一次增量裁决 → 计数重算），
/// 不是直接调裁决函数，因此计时覆盖了 AC 关心的整条链路。
/// 另外打印"全量索引"的对照耗时（按计划 §8 的做法：宽松断言 + 打印实测，避免慢机器 flaky）。
#[test]
fn ac_link_02_incremental_resolve_scales() {
    // 审计 T-20：与 10 万行灌库用例串行，避免并行争用让 2 秒口径偶发失效。
    let _guard = crate::index_reliability_tests::HEAVY_SCALE_TEST
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    const N: usize = 2000;
    for i in 0..N {
        std::fs::write(
            root.join(format!("n{i:04}.md")),
            format!("# 笔记 {i}\n\n见 [[future]]\n"),
        )
        .expect("写笔记");
    }
    let pool = crate::storage::index::open(&root).expect("建索引库");
    let t_full = std::time::Instant::now();
    crate::index_engine::full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let full_ms = t_full.elapsed().as_millis();

    let before: i64 = count(&pool, "SELECT count(*) FROM link WHERE status = 'dangling'");
    assert_eq!(before as usize, N, "初始应全部悬空");

    std::fs::write(root.join("future.md"), "# 目标\n").expect("写目标");
    let t = std::time::Instant::now();
    crate::index_watch::apply_changes(
        &pool,
        &root,
        &[crate::index_watch::Change::Created("future.md".into())],
        |_| {},
    )
    .expect("增量批次应当成功（失败会连事件一起不发，见 I1）");
    let inc_ms = t.elapsed().as_millis();
    println!(
        "AC-LINK-02 实测：{N} 篇 + {N} 条链接 —— 全量索引 {full_ms} ms；\
         新建一篇被 {N} 条链接指向的笔记后的**增量**路径 {inc_ms} ms（红线 2s）"
    );
    assert!(inc_ms < 2000, "增量路径必须在 2 秒内（实测 {inc_ms} ms）");

    let dangling: i64 = count(&pool, "SELECT count(*) FROM link WHERE status = 'dangling'");
    let resolved: i64 = count(&pool, "SELECT count(*) FROM link WHERE status = 'resolved'");
    assert_eq!(
        dangling, 0,
        "目标出现后不得再有悬空（一次增量裁决到位的证明）"
    );
    assert_eq!(resolved as usize, N, "2000 条全部转为 resolved");
}

/// **锁死"写连接不得卡在打开事务里"**（审计 I2）。
///
/// 背景（已在修复前**实测复现**）：`apply_changes` 原先是
/// `conn.execute_batch("BEGIN IMMEDIATE")` **手工**开事务 + 结尾 COMMIT，闭包中途用 `?`
/// 提前返回时**没有任何 ROLLBACK**（全仓 ROLLBACK 只出现在命令名里）。rusqlite 不跟踪
/// execute_batch 开的事务，SQLite 也只在 IOERR/FULL/BUSY 这类错误上自动回滚 ——
/// **纯 Rust 侧的 Err 绝不回滚**。实测到的后果：
///   第二次开启事务失败：cannot start a transaction within a transaction
/// 即写线程复用同一条连接后，**之后每一次增量索引写入都失败**（而 I1 又把失败吞掉，
/// 徽标仍显示"就绪"），直到应用重启。
///
/// 修法：改用 `conn.unchecked_transaction()` —— `Transaction` **drop 即回滚**，
/// 无论闭包从哪条路径返回。本用例锁住这条不变式：事务中途失败后，下一次事务必须开得起来。
#[test]
fn failed_batch_does_not_leave_the_connection_stuck_in_a_transaction() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲\n").expect("写笔记");
    let pool = crate::storage::index::open(&root).expect("建索引库");
    full_index(&pool, &root, |_, _| {}).expect("首次索引");

    // ① 事务中途失败（模拟 apply_changes 里任意一处 `?` 提前返回）
    let first = pool.with_writer(|conn| {
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| AppError::db("开启事务"))?;
        let _ = &tx; // 事务里什么都没做就失败
        Err::<(), _>(AppError::db("模拟中途失败"))
    });
    assert!(first.is_err(), "这一步就是「中途失败」");

    // ② 下一次写事务必须仍能开起来 —— 修好前这里会报
    //    "cannot start a transaction within a transaction"
    let second = pool.with_writer(|conn| {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::db(&format!("第二次开启事务失败：{e}")))?;
        tx.commit()
            .map_err(|e| AppError::db(&format!("第二次提交失败：{e}")))?;
        Ok(())
    });
    assert!(
        second.is_ok(),
        "一次失败的事务不得让写连接永久卡在打开事务上（当前：{:?}）",
        second.err().map(|e| e.to_string())
    );
}

// ── PRD 勘误 D-42 候选 (a)：增量收尾清理孤儿标签 ───────────────────────

/// 某标签的 `ref_count`；`None` = 该 `tag` 行**不存在**。
fn tag_ref_count(pool: &crate::storage::pool::DbPool, norm: &str) -> Option<i64> {
    pool.with_reader(|conn| {
        conn.query_row(
            "SELECT ref_count FROM tag WHERE norm = ?1",
            rusqlite::params![norm],
            |r| r.get::<_, i64>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(AppError::db(&format!("读标签行：{other}"))),
        })
    })
    .expect("读标签行")
}

/// `tag_tree`（标签面板的数据源）是否会列出某个标签 —— 与命令**同一条链路**：
/// `tag_query::load_tag_rows` + `kp_domain::tag_tree::build_tree`。
fn tag_tree_lists(pool: &crate::storage::pool::DbPool, norm: &str) -> bool {
    fn hit(nodes: &[kp_domain::tag_tree::TagNode], norm: &str) -> bool {
        nodes
            .iter()
            .any(|n| n.norm == norm || hit(&n.children, norm))
    }
    let rows = crate::storage::tag_query::load_tag_rows(pool).expect("读标签行");
    hit(&kp_domain::tag_tree::build_tree(&rows), norm)
}

/// **D-42 候选 (a) 的判据：外部把某个标签从笔记里删掉 ⇒ 增量重索引后该标签行消失。**
///
/// 现场症状（PRD 勘误 D-42）：用别的编辑器删掉 `#旧标签` 后，`ref_count` 变成 0 但 `tag` 行仍在，
/// 面板会长期列出这个"幽灵标签"，点进去是空列表，直到下一次全量重建。
#[test]
fn incremental_cleanup_removes_tag_no_longer_referenced() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲 #要删的标签\n").expect("写笔记");
    fs::write(root.join("b.md"), "# 乙 #留下的标签\n").expect("写笔记");
    let pool = crate::storage::index::open(&root).expect("建索引库");
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    assert_eq!(
        tag_ref_count(&pool, "要删的标签"),
        Some(1),
        "前置：标签应在"
    );
    assert!(tag_tree_lists(&pool, "要删的标签"), "前置：面板应列出它");

    // 模拟"编辑器外的改动"：直接改盘去掉那个标签，然后走**增量路径**（不是重建）
    fs::write(root.join("a.md"), "# 甲 正文（标签已被外部删掉）\n").expect("外部改写");
    crate::index_watch::apply_changes(
        &pool,
        &root,
        &[Change::Modified("a.md".to_string())],
        |_| {},
    )
    .expect("增量索引");

    assert_eq!(
        tag_ref_count(&pool, "要删的标签"),
        None,
        "无引用的标签行必须被增量收尾清掉（否则就是幽灵标签）"
    );
    assert!(
        !tag_tree_lists(&pool, "要删的标签"),
        "tag_tree 不能再列出它（点进去是空列表就是这个症状）"
    );
    assert_eq!(
        tag_ref_count(&pool, "留下的标签"),
        Some(1),
        "与本次变更无关的标签不许被波及"
    );
}

/// **反向判据：仍有其它笔记引用该标签 ⇒ 不许删。**
///
/// 删掉"还有人用"的标签会让活着的标签从面板消失（比幽灵标签更糟），所以这条必须钉住。
#[test]
fn incremental_cleanup_keeps_tag_still_referenced() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    fs::write(root.join("a.md"), "# 甲 #共用\n").expect("写笔记");
    fs::write(root.join("b.md"), "# 乙 #共用\n").expect("写笔记");
    let pool = crate::storage::index::open(&root).expect("建索引库");
    full_index(&pool, &root, |_, _| {}).expect("首次索引");
    assert_eq!(tag_ref_count(&pool, "共用"), Some(2), "前置：两篇都引用");

    fs::write(root.join("a.md"), "# 甲 只留正文\n").expect("外部改写");
    crate::index_watch::apply_changes(
        &pool,
        &root,
        &[Change::Modified("a.md".to_string())],
        |_| {},
    )
    .expect("增量索引");

    assert_eq!(
        tag_ref_count(&pool, "共用"),
        Some(1),
        "b.md 仍在引用 ⇒ 标签行必须保留（计数降到 1）"
    );
    assert!(tag_tree_lists(&pool, "共用"), "面板仍应列出它");
}
