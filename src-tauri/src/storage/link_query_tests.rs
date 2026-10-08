//! 链接查询层用例：**合成数据直接入库**（不经解析/索引），逐条钉住 SQL 语义。

use super::*;
use crate::index_engine::full_index;
use crate::storage::index::open;

/// 合成数据行（抽成别名以满足 clippy::type_complexity）。
type FileRow<'a> = (&'a str, &'a str, &'a str, &'a str);
type LinkRow<'a> = (
    &'a str,
    Option<&'a str>,
    &'a str,
    &'a str,
    &'a str,
    i64,
    i64,
);
type OwnedLink = (String, Option<String>, String, String, String, i64, i64);

struct Fixture {
    _dir: tempfile::TempDir,
    pool: DbPool,
}

/// 建一个只含索引库的临时 Vault，并按给定元组写入 file/link 行。
/// file 行：(rel_path, name, stem, kind)；link 行：(src, dst, target_ref, status, kind, line, col)
fn fixture(files: &[FileRow<'_>], links: &[LinkRow<'_>]) -> Fixture {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path();
    for (rel, _, _, _) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("建目录");
        }
        std::fs::write(&path, "# 标题\n\n内容\n").expect("写文件");
    }
    let pool = open(root).expect("建索引库");
    full_index(&pool, root, |_, _| {}).expect("首次索引");
    // `with_writer` 在专用写线程上执行，闭包必须是 'static —— 因此先把数据 clone 成 owned。
    let owned_files: Vec<(String, String, String, String)> = files
        .iter()
        .map(|(a, b, c, d)| (a.to_string(), b.to_string(), c.to_string(), d.to_string()))
        .collect();
    let owned_links: Vec<OwnedLink> = links
        .iter()
        .map(|(src, dst, target, status, kind, line, col)| {
            (
                src.to_string(),
                dst.map(|d| d.to_string()),
                target.to_string(),
                status.to_string(),
                kind.to_string(),
                *line,
                *col,
            )
        })
        .collect();
    pool.with_writer(move |conn| {
        for (rel, name, stem, kind) in &owned_files {
            conn.execute(
                "UPDATE file SET name = ?2, stem = ?3, kind = ?4 WHERE rel_path = ?1",
                rusqlite::params![rel, name, stem, kind],
            )
            .map_err(|_| AppError::db("更新 file"))?;
        }
        conn.execute("DELETE FROM link", [])
            .map_err(|_| AppError::db("清空 link"))?;
        for (src, dst, target_ref, status, kind, line, col) in &owned_links {
            let src_id: i64 = conn
                .query_row(
                    "SELECT id FROM file WHERE rel_path = ?1",
                    rusqlite::params![src],
                    |r| r.get(0),
                )
                .map_err(|_| AppError::db("找源文件"))?;
            let dst_id: Option<i64> = match dst {
                Some(d) => Some(
                    conn.query_row(
                        "SELECT id FROM file WHERE rel_path = ?1",
                        rusqlite::params![d],
                        |r| r.get(0),
                    )
                    .map_err(|_| AppError::db("找目标文件"))?,
                ),
                None => None,
            };
            conn.execute(
                "INSERT INTO link (src_file_id, dst_file_id, target_ref, anchor, alias, link_kind, status, line, col) \
                 VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, ?6, ?7)",
                rusqlite::params![src_id, dst_id, target_ref, kind, status, line, col],
            )
            .map_err(|_| AppError::db("插入 link"))?;
        }
        Ok(())
    })
    .expect("准备数据");
    Fixture { _dir: dir, pool }
}

#[test]
fn backlinks_only_count_resolved_and_are_ordered() {
    let f = fixture(
        &[
            ("B.md", "B", "B", "note"),
            ("甲.md", "甲", "甲", "note"),
            ("乙.md", "乙", "乙", "note"),
        ],
        &[
            ("甲.md", Some("B.md"), "B", "resolved", "wikilink", 3, 5),
            ("乙.md", Some("B.md"), "B", "resolved", "wikilink", 1, 1),
            ("乙.md", None, "缺", "dangling", "wikilink", 9, 1),
        ],
    );
    let rows = backlinks_for(&f.pool, "B.md").expect("反链查询");
    assert_eq!(rows.len(), 2, "只算 resolved 反链");
    assert_eq!(
        rows[0].src_rel_path, "乙.md",
        "按来源路径排序（乙 > 甲 的码位顺序）"
    );
    assert_eq!(rows[0].line, 1);
    assert_eq!(rows[1].src_rel_path, "甲.md");
    assert_eq!(rows[1].line, 3);
    assert_eq!(rows[1].col, 5);
}

#[test]
fn backlinks_of_unknown_target_is_empty_not_error() {
    let f = fixture(&[("A.md", "A", "A", "note")], &[]);
    assert!(backlinks_for(&f.pool, "不存在.md")
        .expect("不应报错")
        .is_empty());
}

#[test]
fn dangling_groups_aggregate_by_target() {
    let f = fixture(
        &[("甲.md", "甲", "甲", "note"), ("乙.md", "乙", "乙", "note")],
        &[
            ("甲.md", None, "X", "dangling", "wikilink", 1, 1),
            ("乙.md", None, "X", "dangling", "wikilink", 2, 1),
            ("乙.md", None, "X", "dangling", "wikilink", 4, 1),
            ("甲.md", None, "Y", "dangling", "wikilink", 5, 1),
        ],
    );
    let g = dangling_groups(&f.pool).expect("悬空分组");
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].target_ref, "X", "按引用次数降序");
    assert_eq!(g[0].ref_count, 3, "X 被引用 3 处");
    assert_eq!(g[0].source_count, 2, "来自 2 篇笔记");
    assert_eq!(g[1].target_ref, "Y");
    assert_eq!(g[1].ref_count, 1);
}

#[test]
fn ambiguous_rows_list_candidates_case_insensitively() {
    let f = fixture(
        &[
            ("甲.md", "甲", "甲", "note"),
            ("folder1/note.md", "note", "note", "note"),
            ("folder2/note.md", "note", "note", "note"),
        ],
        &[("甲.md", None, "note", "ambiguous", "wikilink", 1, 1)],
    );
    let a = ambiguous_rows(&f.pool).expect("歧义查询");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].target_ref, "note");
    assert_eq!(
        a[0].candidates,
        vec!["folder1/note.md".to_string(), "folder2/note.md".to_string()],
        "候选按路径排序，大小写不敏感同名"
    );
}

#[test]
fn orphans_exclude_self_links_and_linked_notes() {
    let f = fixture(
        &[
            ("独立.md", "独立", "独立", "note"),
            ("自链.md", "自链", "自链", "note"),
            ("出链.md", "出链", "出链", "note"),
            ("被引.md", "被引", "被引", "note"),
        ],
        &[
            (
                "自链.md",
                Some("自链.md"),
                "自链",
                "resolved",
                "wikilink",
                1,
                1,
            ),
            (
                "出链.md",
                Some("被引.md"),
                "被引",
                "resolved",
                "wikilink",
                1,
                1,
            ),
        ],
    );
    let o = orphan_notes(&f.pool).expect("孤立查询");
    assert_eq!(
        o,
        vec!["独立.md".to_string(), "自链.md".to_string()],
        "自链接不算出链，也不算出链目标"
    );
}

#[test]
fn headings_are_returned_in_document_order() {
    let f = fixture(&[("A.md", "A", "A", "note")], &[]);
    let h = headings_of(&f.pool, "A.md").expect("标题查询");
    assert_eq!(h.len(), 1, "夹具内容只有一个 # 标题");
    assert_eq!(h[0].text, "标题");
    assert_eq!(h[0].line, 1);
    assert!(headings_of(&f.pool, "无此文件.md")
        .expect("不应报错")
        .is_empty());
}

/// **FR-FILE-21 的候选集**：按词干（大小写不敏感）反查引用了目标的来源文件，
/// 并允许目标写成 folder/名（改写器的匹配口径如此，候选集必须一致，否则会漏改）。
#[test]
fn files_referencing_matches_stem_case_insensitively() {
    let f = fixture(
        &[
            ("a.md", "a.md", "a", "note"),
            ("b.md", "b.md", "b", "note"),
            ("sub/c.md", "c.md", "c", "note"),
        ],
        &[
            ("a.md", Some("b.md"), "B", "resolved", "wikilink", 1, 1),
            (
                "sub/c.md",
                Some("b.md"),
                "folder/B",
                "resolved",
                "wikilink",
                1,
                1,
            ),
            ("b.md", None, "别的目标", "dangling", "wikilink", 1, 1),
        ],
    );
    let hit = files_referencing(&f.pool, "b").expect("查询引用");
    assert_eq!(
        hit,
        vec!["a.md".to_string(), "sub/c.md".to_string()],
        "大小写不同的 B 与带目录的 folder/B 都要命中，且按路径排序"
    );
    assert!(
        files_referencing(&f.pool, "zzz")
            .expect("查询引用")
            .is_empty(),
        "无关词干不得命中"
    );
}

/// FR-LINK-22 的 \`link_resolve_ambiguous\` 靠它定位：给定 link.id 必须能拿到
/// **来源文件 + 1-based 行列 + 原目标**；不存在的 id 返回 None（而不是报错，由命令层决定怎么提示）。
#[test]
fn link_by_id_returns_source_and_position() {
    let f = fixture(
        &[("a.md", "a.md", "a", "note"), ("b.md", "b.md", "b", "note")],
        &[("a.md", Some("b.md"), "note", "ambiguous", "wikilink", 7, 3)],
    );
    let id: i64 = f
        .pool
        .with_reader(|conn| {
            conn.query_row("SELECT id FROM link WHERE target_ref = 'note'", [], |r| {
                r.get(0)
            })
            .map_err(|e| kp_domain::error::AppError::db(&format!("取 id 失败：{e}")))
        })
        .expect("取 id");
    let row = link_by_id(&f.pool, id).expect("查询").expect("应命中");
    assert_eq!(row.0, "a.md", "来源文件");
    assert_eq!((row.1, row.2), (7, 3), "行列必须是 1-based 且与索引一致");
    assert_eq!(row.3, "note", "原目标");
    assert!(
        link_by_id(&f.pool, 999_999).expect("查询").is_none(),
        "不存在的 id 应为 None"
    );
}

/// **FR-LINK-22 / AC-LINK-05 的关键接口**：歧义行必须带上各条的 **link.id** ——
/// 面板要逐条"指定目标"，而 \`link_resolve_ambiguous { link_id, target_rel_path }\` 收的正是 link_id。
/// 没有它，UI 只能"整组"处理，做不到 AC-LINK-04 的"为其中一条指定"。
#[test]
fn ambiguous_rows_carry_link_ids() {
    let f = fixture(
        &[
            ("f1/note.md", "note.md", "note", "note"),
            ("f2/note.md", "note.md", "note", "note"),
            ("src.md", "src.md", "src", "note"),
        ],
        &[
            ("src.md", None, "note", "ambiguous", "wikilink", 1, 1),
            ("src.md", None, "note", "ambiguous", "wikilink", 3, 1),
        ],
    );
    let rows = ambiguous_rows(&f.pool).expect("查询歧义");
    assert_eq!(rows.len(), 1, "同目标名归一组");
    assert_eq!(rows[0].ref_count, 2);
    assert_eq!(rows[0].link_ids.len(), 2, "两条链接的 id 都要带上");
    // 每个 id 都必须能反查到来源与位置（命令层就是靠这个定位的）
    for id in &rows[0].link_ids {
        let row = link_by_id(&f.pool, *id)
            .expect("按 id 查")
            .expect("必须存在");
        assert_eq!(row.0, "src.md");
        assert!(row.1 >= 1 && row.2 >= 1, "行列应为 1-based");
    }
    assert!(
        rows[0].link_ids[0] != rows[0].link_ids[1],
        "两个 id 不应相同"
    );
}
