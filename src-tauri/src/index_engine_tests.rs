//! 索引引擎测试：**真实临时 Vault**（不手搓 SQL、不猜 DDL —— 走 `index_file` 的真实数据面）。

use std::fs;
use std::path::Path;

use crate::index_engine::{full_index, scan_vault, MAX_NOTE_BYTES};
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
    let expr = kp_domain::search::build_match_expr("段落内容", kp_domain::search::MatchMode::All);
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
