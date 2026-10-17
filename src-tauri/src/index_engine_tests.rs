//! 索引引擎测试：**真实临时 Vault**（不手搓 SQL、不猜 DDL —— 走 `index_file` 的真实数据面）。

use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::index_engine::{full_index, full_index_with_timeout, scan_vault, IndexMode};
use crate::index_scan::MAX_NOTE_BYTES;
use crate::storage::index::open;
use crate::storage::pool::DbPool;

fn write_note(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).expect("建目录");
    }
    fs::write(p, content).expect("写笔记");
}

/// 三篇笔记 + 两个同名 stem（用于歧义）+ 应被忽略的目录与非 Markdown 文件。
fn setup_vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path();
    write_note(
        root,
        "a.md",
        "---\ntags: [项目/甲]\n---\n# 标题甲\n\n正文 [[b]] 与 [[missing]] 与 [[c]]\n\n#标签一\n\n段落内容 ^blk1\n",
    );
    write_note(root, "b.md", "# 标题乙\n\n指向 [[a|别名]]\n");
    write_note(root, "sub/c.md", "# 子目录\n\n同名之一\n");
    write_note(root, "other/c.md", "# 另一个 \n\n同名之二\n");
    write_note(root, ".obsidian/x.md", "# 不应索引\n");
    write_note(root, ".knowlpad/y.md", "# 不应索引\n");
    write_note(root, "note.txt", "非 Markdown\n");
    dir
}

fn count(pool: &DbPool, sql: &str) -> i64 {
    pool.with_reader(|conn| {
        conn.query_row(sql, [], |r| r.get(0))
            .map_err(|_| kp_domain::error::AppError::db("计数"))
    })
    .expect("计数查询")
}

fn counts(pool: &DbPool) -> Vec<i64> {
    vec![
        count(pool, "SELECT count(*) FROM file"),
        count(pool, "SELECT count(*) FROM note_fts"),
        count(pool, "SELECT count(*) FROM heading"),
        count(pool, "SELECT count(*) FROM block_id"),
        count(pool, "SELECT count(*) FROM link"),
        count(pool, "SELECT count(*) FROM tag"),
        count(pool, "SELECT count(*) FROM file_tag"),
    ]
}

#[test]
fn full_index_writes_all_entity_tables() {
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");
    let outcome = full_index(&pool, dir.path(), |_, _| {}).expect("全量索引");
    assert_eq!(
        outcome.indexed, 5,
        "四篇笔记 + 一个附件（PR-6：附件也要进文件树）"
    );
    let c = counts(&pool);
    assert_eq!(c[0], 5, "file 行数（四篇笔记 + note.txt 附件）");
    assert_eq!(c[1], 4, "note_fts 只对笔记建行");
    assert!(c[2] >= 4, "标题应入库：{c:?}");
    assert_eq!(c[3], 1, "一个块 ID");
    assert_eq!(c[4], 4, "四条链接（b/missing/c/别名）");
    // 标签共 3 个：项目/甲（frontmatter，含祖先 项目）+ 标签一；file_tag 与展开条目一一对应
    assert_eq!(c[5], 3, "标签含层级展开：{c:?}");
    assert_eq!(c[6], 3, "文件-标签关联：{c:?}");
}

#[test]
fn scan_skips_hidden_dirs_and_tmp_but_keeps_attachments() {
    let dir = setup_vault();
    write_note(dir.path(), ".kp-tmp-abc.md", "临时文件\n");
    let (entries, _) = scan_vault(dir.path()).expect("遍历");
    let paths: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
    assert_eq!(paths.len(), 5, "四篇笔记 + note.txt（附件）：{paths:?}");
    assert!(!paths
        .iter()
        .any(|p| p.contains(".obsidian") || p.contains(".knowlpad")));
    assert!(
        paths.iter().any(|p| p.ends_with(".txt")),
        "PR-6：非 Markdown 要进索引"
    );
    assert!(!paths.iter().any(|p| p.contains(".kp-tmp-")));
    assert!(!paths.iter().any(|p| p.contains(".kp-tmp-")));
}

#[test]
fn oversize_note_is_skipped_with_warning() {
    let dir = setup_vault();
    let big = "x".repeat((MAX_NOTE_BYTES + 1) as usize);
    write_note(dir.path(), "big.md", &big);
    let (entries, warnings) = scan_vault(dir.path()).expect("遍历");
    assert!(
        !entries.iter().any(|e| e.rel_path == "big.md"),
        "超 5MB 应跳过"
    );
    assert!(
        warnings.iter().any(|w| w.contains("skip-oversize")),
        "应记警告"
    );
}

/// **DEBT-32 / SEC-03 的端到端判据**：单文件解析超时 ⇒ **跳过该文件 + 记 `skip-timeout:` 警告**，
/// 且**一行都不写**（"不留半成品"）。
///
/// 用零超时打中这条分支是确定的：超时的**后果**（跳过 / 警告 / 零写入）与"等了多久"无关；
/// 真的等 10 秒的用例不会有人跑。注入点见 `full_index_with_timeout`。
#[test]
fn timed_out_file_is_skipped_without_partial_rows() {
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");

    // ① 生产默认超时（10s）⇒ 照旧全部写入，且没有超时警告
    let ok = full_index(&pool, dir.path(), |_, _| {}).expect("首次索引");
    assert_eq!(ok.indexed, 5, "正常路径行为不得改变");
    assert!(
        ok.warnings.is_empty(),
        "正常路径不该有警告：{:?}",
        ok.warnings
    );
    let before = counts(&pool);
    assert_eq!(before[0], 5, "file 行数（四篇笔记 + 一个附件）");

    // ② 同一批文件零超时重跑（Force）：四篇笔记全部超时 ⇒ 不计入 indexed、也**没有任何行被改动**
    let timed = full_index_with_timeout(
        &pool,
        dir.path(),
        |_, _| {},
        IndexMode::Force,
        || false,
        Duration::ZERO,
    )
    .expect("零超时索引");
    assert_eq!(
        timed.indexed, 1,
        "只有不解析的附件会写入；四篇笔记必须被跳过"
    );
    assert_eq!(
        timed.warnings.len(),
        4,
        "四篇笔记各一条警告：{:?}",
        timed.warnings
    );
    assert!(
        timed
            .warnings
            .iter()
            .all(|w| w.starts_with("skip-timeout:")),
        "警告必须带可诊断前缀（与 skip-oversize: 同口径）：{:?}",
        timed.warnings
    );
    assert_eq!(
        counts(&pool),
        before,
        "超时不得改动任何一行：旧行必须保留（而不是先删后失败）"
    );

    // ③ 空库 + 零超时：除了附件那条 file 行，不该有任何派生行（半成品的定义）
    let dir2 = setup_vault();
    let pool2 = open(dir2.path()).expect("建索引库");
    let fresh = full_index_with_timeout(
        &pool2,
        dir2.path(),
        |_, _| {},
        IndexMode::Force,
        || false,
        Duration::ZERO,
    )
    .expect("零超时索引");
    assert_eq!(fresh.indexed, 1, "只有附件（不解析）会写入");
    assert_eq!(
        counts(&pool2),
        vec![1, 0, 0, 0, 0, 0, 0],
        "超时后不得留下任何派生行（file / note_fts / heading / block_id / link / tag / file_tag）"
    );
}

#[test]
fn reindex_is_idempotent() {
    // AC-REL-03 的核心：重复索引不得产生重复行
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("第一次");
    let first = counts(&pool);
    full_index(&pool, dir.path(), |_, _| {}).expect("第二次");
    let second = counts(&pool);
    assert_eq!(first, second, "两次索引的实体计数必须一致");
}

#[test]
fn updated_content_replaces_old_rows() {
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("第一次");
    let before = count(&pool, "SELECT count(*) FROM heading");
    write_note(dir.path(), "b.md", "# 标题乙\n\n# 新增标题\n\n指向 [[a]]\n");
    full_index(&pool, dir.path(), |_, _| {}).expect("第二次");
    let after = count(&pool, "SELECT count(*) FROM heading");
    assert_eq!(
        after,
        before + 1,
        "改后的标题数应 +1（旧行被清掉，不是累加）"
    );
}

#[test]
fn fts_query_finds_chinese_content_through_the_engine() {
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("全量索引");
    let expr = kp_domain::search::build_match_expr("段落内容", kp_domain::search::MatchMode::All)
        .expect("构造 MATCH");
    let hits: i64 = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT count(*) FROM note_fts WHERE note_fts MATCH ?1",
                rusqlite::params![expr],
                |r| r.get(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("FTS 查询"))
        })
        .expect("FTS 查询");
    assert!(hits >= 1, "索引后应能按中文检索到（expr = {expr}）");
}

#[test]
fn link_resolution_sets_resolved_dangling_and_ambiguous() {
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("全量索引");
    let status = |target: &str| -> String {
        pool.with_reader(|conn| {
            conn.query_row(
                "SELECT status FROM link WHERE target_ref = ?1",
                rusqlite::params![target],
                |r| r.get(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("读裁决"))
        })
        .expect("读裁决")
    };
    assert_eq!(status("b"), "resolved", "唯一同名 → resolved");
    assert_eq!(status("missing"), "dangling", "无候选 → dangling");
    assert_eq!(
        status("c"),
        "ambiguous",
        "两个同名 stem → ambiguous（不猜）"
    );
    assert_eq!(status("a"), "resolved", "别名/路径匹配到 a.md");
}

#[test]
fn tag_ref_count_is_recomputed() {
    let dir = setup_vault();
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("全量索引");
    let refs = |norm: &str| -> i64 {
        pool.with_reader(|conn| {
            conn.query_row(
                "SELECT ref_count FROM tag WHERE norm = ?1",
                rusqlite::params![norm],
                |r| r.get(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("读标签"))
        })
        .expect("读标签")
    };
    assert_eq!(refs("标签一"), 1);
    assert_eq!(refs("项目/甲"), 1, "frontmatter 层级标签自身");
    assert_eq!(refs("项目"), 1, "祖先层级也被引用一次");
}
/// PR-6：附件（非 Markdown）进索引但**不进全文索引**，kind 记为 attachment。
#[test]
fn attachment_is_indexed_without_full_text() {
    let dir = setup_vault();
    write_note(dir.path(), "图片.png", "not really a png");
    let pool = open(dir.path()).expect("建索引库");
    full_index(&pool, dir.path(), |_, _| {}).expect("全量索引");
    let kind: String = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT kind FROM file WHERE rel_path = '图片.png'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("读 kind"))
        })
        .expect("附件应已入库");
    assert_eq!(kind, "attachment");
    let fts: i64 = count(&pool, "SELECT count(*) FROM note_fts");
    let files: i64 = count(&pool, "SELECT count(*) FROM file");
    assert_eq!(files, 6, "四篇笔记 + note.txt + 图片.png");
    assert_eq!(fts, 4, "只有四篇 .md 进全文索引，附件不进");
}

/// **MD-WL-04 矩阵**：同目录优先 → 更浅路径优先 → 等深并列仍 ambiguous（不猜）。
#[test]
fn md_wl_04_disambiguation_prefers_same_dir_then_shallower_path() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path();
    // 三个同名笔记，深度分别为 0 / 1 / 2
    write_note(root, "note.md", "# 根\n");
    write_note(root, "sub/note.md", "# 一层\n");
    write_note(root, "deep/x/note.md", "# 两层\n");
    // 三个不同目录的来源，各自指向 [[note]]
    write_note(root, "sub/src.md", "见 [[note]]\n");
    write_note(root, "deep/x/src.md", "见 [[note]]\n");
    write_note(root, "elsewhere/src.md", "见 [[note]]\n");
    let pool = open(root).expect("建索引库");
    full_index(&pool, root, |_, _| {}).expect("全量索引");
    let judge_of = |src: &str| -> (String, Option<String>) {
        pool.with_reader(|conn| {
            conn.query_row(
                "SELECT l.status, d.rel_path FROM link l \
                 JOIN file f ON f.id = l.src_file_id \
                 LEFT JOIN file d ON d.id = l.dst_file_id \
                 WHERE f.rel_path = ?1 AND l.target_ref = 'note'",
                rusqlite::params![src],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .map_err(|_| kp_domain::error::AppError::db("读裁决"))
        })
        .expect("读裁决")
    };
    // ① 同目录优先：sub/src.md 应命中 sub/note.md（尽管根部的 note.md 更浅）
    let (st, dst) = judge_of("sub/src.md");
    assert_eq!(st, "resolved", "同目录候选应胜出");
    assert_eq!(dst.as_deref(), Some("sub/note.md"));
    // ② 同目录优先：deep/x/src.md 应命中 deep/x/note.md（尽管它最深）
    let (st, dst) = judge_of("deep/x/src.md");
    assert_eq!(st, "resolved", "同目录候选应胜出（即使它更深）");
    assert_eq!(dst.as_deref(), Some("deep/x/note.md"));
    // ③ 无同目录候选 → 取更浅者：elsewhere/ 的链接应命中根部的 note.md
    let (st, dst) = judge_of("elsewhere/src.md");
    assert_eq!(st, "resolved", "无同目录候选时应取路径更浅者");
    assert_eq!(dst.as_deref(), Some("note.md"));
    // ④ 等深并列 → 不猜（构造两个等深的同名笔记，来源在第三个目录）
    write_note(root, "p1/dup.md", "# 甲\n");
    write_note(root, "p2/dup.md", "# 乙\n");
    write_note(root, "p3/srcdup.md", "见 [[dup]]\n");
    full_index(&pool, root, |_, _| {}).expect("再次索引");
    let dup = pool
        .with_reader(|conn| {
            conn.query_row(
                "SELECT l.status FROM link l JOIN file f ON f.id = l.src_file_id \
                 WHERE f.rel_path = 'p3/srcdup.md' AND l.target_ref = 'dup'",
                [],
                |r| r.get::<_, String>(0),
            )
            .map_err(|_| kp_domain::error::AppError::db("读裁决"))
        })
        .expect("读裁决");
    assert_eq!(dup, "ambiguous", "等深并列不得猜");
}

/// **标签写入的语义钉子**（2026-10-15 引入，2026-10-16 按裁定改为"两半"）：
///
/// **display 首次写入者胜、结构字段最后一次写入者胜** —— 依据分开写清楚：
/// - `display` 由 **AC-TAG-03 / FR-TAG-09** 定为「**首次**出现的原文形式」⇒ 冲突时不覆盖；
/// - `depth` / `is_leaf` 由 **AC-TAG-01** 定为反映**最终**层级（"仅最末级为真"）⇒ 照旧最后一次胜。
///
/// 这条同时是**拒绝**「进程内缓存 norm→tag_id」的理由：缓存会让**结构字段**停在首次写入的形态
/// （例如 `work` 停在 `is_leaf = 1`），与最终层级不符 ⇒ 最终行不再逐字节一致。
///
/// 另覆盖：MD-TAG-03 的祖先展开、同一文件每次出现各留一行 `file_tag`。
#[test]
fn tag_write_semantics_display_first_wins_structure_last_wins() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path();
    // 索引顺序按 rel_path 升序：a.md 在前、b.md 在后。
    // display：**首次**（a.md 的 "Work"）胜；结构字段：**最后**（b.md 的层级）胜。
    write_note(root, "a.md", "#Work 与 #Work/子 出现两次 #Work\n");
    write_note(root, "b.md", "#work/子/孙\n");
    let pool = open(root).expect("建索引库");
    full_index(&pool, root, |_, _| {}).expect("全量索引");

    let tag_row = |norm: &str| -> (String, i64) {
        pool.with_reader(|conn| {
            conn.query_row(
                "SELECT display, is_leaf FROM tag WHERE norm = ?1",
                rusqlite::params![norm],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map_err(|e| kp_domain::error::AppError::db_with("读 tag", &e))
        })
        .unwrap_or_else(|e| panic!("读 tag({norm}) 失败：{e}"))
    };

    assert_eq!(
        tag_row("work").0,
        "Work",
        "display 由**首次**写入决定（a.md 的 #Work）—— AC-TAG-03 / FR-TAG-09 的原文要求"
    );
    assert_eq!(
        tag_row("work").1,
        0,
        "is_leaf 由**最后一次**写入决定（b.md 的 #work/子/孙 让 work 有后代）—— AC-TAG-01；缓存方案会停在首次的 1"
    );
    assert_eq!(
        tag_row("work/子").0,
        "Work/子",
        "祖先链的 display 同样取**首次**出现的大小写（a.md 的 #Work/子）"
    );
    assert_eq!(tag_row("work/子/孙").1, 1, "最末级 is_leaf = 1");

    // file_tag 逐次出现各一行：a.md = Work(1) + Work/子(2：自身+祖先) + Work(1) = 4；b.md = work/子/孙 展开 3 = 3
    assert_eq!(
        count(&pool, "SELECT count(*) FROM file_tag"),
        7,
        "每次出现都要各留一行 file_tag（MD-TAG-03 展开后）"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM tag"),
        3,
        "祖先链展开成独立行：work、work/子、work/子/孙"
    );
}

/// **D-41：重建收尾清理孤儿 tag 行**（没有任何 `file_tag` 引用的 `tag` 行）。
///
/// 依据两条：AC-TAG-02 末句「`#old` 若无其他子孙则从标签表中消失」＋ AC-REL-03 的"重建与一次性索引逐表一致"
/// （孤儿行会让两者不同 ⇒ 属正确性缺陷）。修法是最小改法：重建收尾一条 DELETE，只在**未取消**的全量重建末尾执行。
#[test]
fn rebuild_purges_orphan_tag_rows() {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path();
    write_note(root, "a.md", "#old 甲\n#keep 乙\n");
    let pool = open(root).expect("建索引库");
    full_index(&pool, root, |_, _| {}).expect("首次索引");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM tag WHERE norm = 'old'"),
        1
    );

    // 模拟"外部把标签删掉"（用别的工具改文件），然后重建
    write_note(root, "a.md", "没有标签了\n");
    full_index(&pool, root, |_, _| {}).expect("重建");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM tag WHERE norm = 'old'"),
        0,
        "孤儿标签行必须被清理（否则 tag_tree 会列出点进去为空的幽灵标签）"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM tag WHERE norm = 'keep'"),
        0,
        "两个标签都没有引用了"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM file_tag"),
        0,
        "引用行同样清空"
    );
}
