//! 索引引擎测试：**真实临时 Vault**（不手搓 SQL、不猜 DDL —— 走 `index_file` 的真实数据面）。

use std::fs;
use std::path::Path;

use crate::index_engine::{full_index, scan_notes, MAX_NOTE_BYTES};
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
    assert_eq!(outcome.indexed, 4, "a/b/sub-c/other-c 四篇");
    let c = counts(&pool);
    assert_eq!(c[0], 4, "file 行数");
    assert_eq!(c[1], 4, "note_fts 与 file 一一对应");
    assert!(c[2] >= 4, "标题应入库：{c:?}");
    assert_eq!(c[3], 1, "一个块 ID");
    assert_eq!(c[4], 4, "四条链接（b/missing/c/别名）");
    // 标签共 3 个：项目/甲（frontmatter，含祖先 项目）+ 标签一；file_tag 与展开条目一一对应
    assert_eq!(c[5], 3, "标签含层级展开：{c:?}");
    assert_eq!(c[6], 3, "文件-标签关联：{c:?}");
}

#[test]
fn scan_skips_hidden_dirs_tmp_and_non_markdown() {
    let dir = setup_vault();
    write_note(dir.path(), ".kp-tmp-abc.md", "临时文件\n");
    let (entries, _) = scan_notes(dir.path()).expect("遍历");
    let paths: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
    assert_eq!(paths.len(), 4, "实际：{paths:?}");
    assert!(!paths
        .iter()
        .any(|p| p.contains(".obsidian") || p.contains(".knowlpad")));
    assert!(!paths.iter().any(|p| p.ends_with(".txt")));
    assert!(!paths.iter().any(|p| p.contains(".kp-tmp-")));
}

#[test]
fn oversize_note_is_skipped_with_warning() {
    let dir = setup_vault();
    let big = "x".repeat((MAX_NOTE_BYTES + 1) as usize);
    write_note(dir.path(), "big.md", &big);
    let (entries, warnings) = scan_notes(dir.path()).expect("遍历");
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
