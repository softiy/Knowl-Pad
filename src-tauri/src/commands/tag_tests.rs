//! 标签域命令的测试（判据打在命令体上；DEBT-14 的做法：命令体可直接用真实临时库调用）。
//!
//! 覆盖 PRD §5.3.6 / §5.3.6.1 / §5.3.6.2 的 `tag_tree` 与 `tag_notes`，判据对着**验收原文**：
//! AC-TAG-01（树形 + 各级计数 = 自身及全部子孙 + is_leaf 仅末级）、AC-TAG-03（大小写去重）、
//! AC-TAG-04（逐标签 COUNT(*) FROM file_tag 与 tag.ref_count 一致）、
//! FR-TAG-10 **主体**（frontmatter 的 tags 与正文标签**统一计数**）、FR-TAG-13（未分类条目）。

use crate::commands::tag::{tag_notes_impl, tag_tree_impl, TagNotesArgs};
use crate::commands::tag_view::TagNode;
use crate::state::AppState;

/// 建临时 Vault（真实文件）+ 真实全量索引，返回 AppState（与 search_tests 的夹具同源）。
fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, AppState) {
    let vault = tempfile::tempdir().expect("Vault 临时目录");
    let root = vault.path().to_path_buf();
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).expect("建目录");
        }
        std::fs::write(&path, content).expect("写文件");
    }
    let pool = crate::storage::index::open(&root).expect("建索引库");
    crate::index_engine::full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let config = tempfile::tempdir().expect("配置目录");
    let global = crate::storage::global::open(config.path()).expect("建全局库");
    let state = AppState::new();
    state.set_root(Some(root));
    state.set_index_db(Some(std::sync::Arc::new(pool)));
    state.set_global_db(std::sync::Arc::new(global));
    (vault, state)
}

/// 在树里按 norm 找节点（**递归**：层级标签是子节点，只查根会漏）。
fn find_node<'a>(nodes: &'a [TagNode], norm: &str) -> Option<&'a TagNode> {
    for node in nodes {
        if node.norm == norm {
            return Some(node);
        }
        if let Some(hit) = find_node(&node.children, norm) {
            return Some(hit);
        }
    }
    None
}

/// 同上的断言版（找不到时把现有 norm 全列出来，便于定位）。
fn root_node<'a>(tree: &'a [TagNode], norm: &str) -> &'a TagNode {
    find_node(tree, norm).unwrap_or_else(|| {
        fn collect<'b>(nodes: &'b [TagNode], out: &mut Vec<&'b str>) {
            for n in nodes {
                out.push(&n.norm);
                collect(&n.children, out);
            }
        }
        let mut all = Vec::new();
        collect(tree, &mut all);
        panic!("树里没有 {norm}（现有：{all:?}）")
    })
}

fn notes(state: &AppState, tag_norm: &str, include_children: bool) -> Vec<String> {
    tag_notes_impl(
        state,
        TagNotesArgs {
            tag_norm: tag_norm.to_string(),
            include_children,
        },
    )
    .expect("查询标签笔记")
    .into_iter()
    .map(|n| n.rel_path)
    .collect()
}

/// **AC-TAG-01 原文即用例**：`#项目/alpha/前端`、`#项目/alpha/后端`、`#项目/beta`、`#随笔`
/// ⇒ 树形「项目 → alpha → {前端, 后端}、项目 → beta、随笔」；各级计数 = 自身及全部子孙；is_leaf 仅末级为真。
#[test]
fn ac_tag_01_tree_shape_counts_and_is_leaf() {
    let (_v, state) = fixture(&[
        ("a.md", "#项目/alpha/前端 甲\n"),
        ("b.md", "#项目/alpha/前端 乙\n"),
        ("c.md", "#项目/alpha/后端 丙\n"),
        ("d.md", "#项目/beta 丁\n"),
        ("e.md", "#随笔 戊\n#随笔 己\n"),
    ]);
    let tree = tag_tree_impl(&state).expect("标签树");

    let project = root_node(&tree, "项目");
    let kids: Vec<&str> = project.children.iter().map(|n| n.norm.as_str()).collect();
    assert_eq!(kids, vec!["项目/alpha", "项目/beta"]);
    let alpha = &project.children[0];
    let alpha_kids: Vec<&str> = alpha.children.iter().map(|n| n.norm.as_str()).collect();
    assert_eq!(alpha_kids, vec!["项目/alpha/前端", "项目/alpha/后端"]);

    // **计数口径 = file_tag 行数**（AC-TAG-04/FR-TAG-12 的原文定义），且 MD-TAG-03 的祖先登记
    // 让祖先天然含子孙 ⇒ 这里"项目"的 4 = 4 篇笔记各写了一次祖先行（不是把子孙加起来）。
    assert_eq!(project.ref_count, 4, "4 篇笔记的祖先登记各写一行");
    assert_eq!(alpha.ref_count, 3, "前端 2 + 后端 1（祖先行）");
    assert_eq!(
        alpha.children[0].ref_count, 2,
        "#项目/alpha/前端 被两篇引用"
    );
    assert_eq!(alpha.children[1].ref_count, 1);

    assert!(!project.is_leaf);
    assert!(!alpha.is_leaf);
    assert!(alpha.children[0].is_leaf);
    assert!(alpha.children[1].is_leaf);
    // 同一篇里写两次 = **两行** ⇒ 计数 2（行数口径；篇数见 tag_notes 的列表长度）
    assert_eq!(
        root_node(&tree, "随笔").ref_count,
        2,
        "同一篇里出现两次算两行"
    );
    assert_eq!(notes(&state, "随笔", false).len(), 1, "但笔记列表只有 1 篇");
}

/// **AC-TAG-03**：`#Work` / `#work` / `#WORK` ⇒ 仅 **1** 个标签条目、`ref_count = 3`、
/// `norm = work`、点击列出全部 3 篇笔记。
#[test]
fn ac_tag_03_case_insensitive_dedup_and_notes_listing() {
    let (_v, state) = fixture(&[
        ("a.md", "#Work 甲\n"),
        ("b.md", "#work 乙\n"),
        ("c.md", "#WORK 丙\n"),
    ]);
    let tree = tag_tree_impl(&state).expect("标签树");
    let work: Vec<&TagNode> = tree.iter().filter(|n| n.norm == "work").collect();
    assert_eq!(work.len(), 1, "大小写不同只算一个条目");
    assert_eq!(work[0].ref_count, 3, "AC-TAG-03：ref_count = 3");
    assert_eq!(work[0].norm, "work", "AC-TAG-03：norm 是小写规范键");
    assert_eq!(notes(&state, "work", false), vec!["a.md", "b.md", "c.md"]);
}

/// **AC-TAG-03 / FR-TAG-09：`display` 取「首次出现」的原文形式**（2026-10-16 裁定①选 (a) 修正后）。
///
/// 曾经的冲突：索引写入原本是"最后一次写入者胜"（`ON CONFLICT DO UPDATE SET display = excluded.display`），
/// 三篇 `#Work`/`#work`/`#WORK` 实测得到 `WORK`，与验收原文相反。
/// 现改为**冲突时不覆盖 display**（结构字段仍最后一次胜，见 `index_sql.rs` 的 `TAG_UPSERT_SQL`）
/// ⇒ 首次出现的 `Work` 胜出。语义钉子见 `index_engine_tests::tag_write_semantics_display_first_wins_structure_last_wins`。
#[test]
fn ac_tag_03_display_is_the_first_occurrence() {
    let (_v, state) = fixture(&[
        ("a.md", "#Work 甲\n"),
        ("b.md", "#work 乙\n"),
        ("c.md", "#WORK 丙\n"),
    ]);
    let tree = tag_tree_impl(&state).expect("标签树");
    let work = root_node(&tree, "work");
    assert_eq!(
        work.display, "Work",
        "AC-TAG-03 / FR-TAG-09：首次出现的形式（a.md 的 #Work）"
    );
}

/// **FR-TAG-10 的来源可区分（2026-10-17 批次 4 落地）**：`tag_notes` 的每一行都带 `sources`，
/// 取值固定顺序 `body` → `frontmatter`；两种来源都写了同一标签时**两个都在**（去重后的并集）。
#[test]
fn fr_tag_10_note_list_reports_sources_per_note() {
    let (_v, state) = fixture(&[
        ("body.md", "#共享 正文\n"),
        ("fm.md", "---\ntags: [共享]\n---\n正文无标签\n"),
        ("both.md", "---\ntags: [共享]\n---\n#共享 又一次\n"),
    ]);
    let rows = tag_notes_impl(
        &state,
        TagNotesArgs {
            tag_norm: "共享".to_string(),
            include_children: false,
        },
    )
    .expect("查询标签笔记");
    let got: Vec<(String, Vec<String>)> =
        rows.into_iter().map(|n| (n.rel_path, n.sources)).collect();
    assert_eq!(
        got,
        vec![
            ("body.md".to_string(), vec!["body".to_string()]),
            (
                "both.md".to_string(),
                vec!["body".to_string(), "frontmatter".to_string()],
            ),
            ("fm.md".to_string(), vec!["frontmatter".to_string()]),
        ],
        "来源必须逐篇给出，顺序固定 body → frontmatter（前端据此渲染两个徽标）"
    );
}

/// **AC-TAG-04**：对**全部**标签断言 SELECT COUNT(*) FROM file_tag WHERE tag_id = ? 与 tag.ref_count 完全一致。
#[test]
fn ac_tag_04_ref_count_matches_file_tag_for_every_tag() {
    let (_v, state) = fixture(&[
        ("a.md", "#项目/alpha/前端 甲\n#work 甲\n"),
        ("b.md", "#项目/alpha/前端 乙\n#Work 乙\n"),
        ("c.md", "#项目/beta 丙\n#随笔 丙\n"),
        ("d.md", "没有任何标签\n"),
    ]);
    let tree = tag_tree_impl(&state).expect("标签树");
    let pool = state.index_db().expect("索引库");
    let mut checked = 0;
    fn walk(nodes: &[TagNode], pool: &crate::storage::pool::DbPool, checked: &mut usize) {
        for node in nodes {
            if node.unclassified {
                continue;
            }
            let actual: i64 = pool
                .with_reader(|c| {
                    c.query_row(
                        "SELECT count(*) FROM file_tag ft JOIN tag t ON t.id = ft.tag_id WHERE t.norm = ?1",
                        rusqlite::params![node.norm],
                        |r| r.get(0),
                    )
                    .map_err(|e| kp_domain::error::AppError::db_with("统计 file_tag", &e))
                })
                .expect("统计 file_tag");
            assert_eq!(
                actual, node.ref_count,
                "AC-TAG-04：标签 {} 的 ref_count 与 file_tag 实际计数必须一致",
                node.norm
            );
            *checked += 1;
            walk(&node.children, pool, checked);
        }
    }
    walk(&tree, &pool, &mut checked);
    assert!(checked >= 4, "至少要真的检查到 4 个标签（实得 {checked}）");
}

/// **FR-TAG-10 主体**：frontmatter 的 tags: 与正文标签**统一纳入计数**（数据面已就绪，本轮断言它）。
///
/// 注意：**来源区分**（"可在笔记的标签列表中区分标识"）本轮不做 —— file_tag 没有 source 列，顺延批次 4（PRD 勘误 D-39②）。
#[test]
fn fr_tag_10_frontmatter_and_body_tags_are_counted_together() {
    let (_v, state) = fixture(&[
        ("fm.md", "---\ntags: [共享, 独有/子]\n---\n正文没有标签\n"),
        ("body.md", "#共享 正文标签\n"),
        ("both.md", "---\ntags: [共享]\n---\n#共享 又一次\n"),
    ]);
    let tree = tag_tree_impl(&state).expect("标签树");
    let shared = root_node(&tree, "共享");
    // 行数口径：fm 1 + body 1 + both 2（frontmatter 与正文各一次）= 4 行；**篇数**是 3。
    assert_eq!(shared.ref_count, 4, "行数：1 + 1 + 2");
    assert_eq!(
        notes(&state, "共享", false),
        vec!["body.md", "both.md", "fm.md"],
        "两种来源都要出现在笔记列表里（这里是**篇数**口径）"
    );
    assert_eq!(
        shared.ref_count as usize,
        notes(&state, "共享", false).len() + 1,
        "4 行 vs 3 篇"
    );
    assert_eq!(
        root_node(&tree, "独有/子").ref_count,
        1,
        "frontmatter 的层级标签同样展开"
    );
    assert_eq!(
        root_node(&tree, "独有").ref_count,
        1,
        "祖先也纳入（祖先登记）"
    );
}

/// **FR-TAG-13**：「未分类」条目在树里可见，计数 = 无标签笔记数，且**排在最后**。
#[test]
fn fr_tag_13_unclassified_entry_is_visible_and_last() {
    let (_v, state) = fixture(&[
        ("tagged.md", "#有标签 甲\n"),
        ("plain1.md", "没有标签\n"),
        ("plain2.md", "也没有\n"),
    ]);
    let tree = tag_tree_impl(&state).expect("标签树");
    let last = tree.last().expect("非空");
    assert!(last.unclassified, "未分类条目必须排在最后");
    assert_eq!(last.norm, "");
    assert_eq!(last.display, "未分类");
    // 未分类条目的计数是**篇数**（它没有 file_tag 行 —— 单位不同，已在契约里写明）
    assert_eq!(last.ref_count, 2, "两篇无标签笔记");
    assert_eq!(notes(&state, "", false), vec!["plain1.md", "plain2.md"]);
    assert_eq!(notes(&state, "有标签", false), vec!["tagged.md"]);
}

/// **FR-TAG-03 的「可切换是否含子级」自 2026-10-17（批次 4）起真正生效**（勘误 **D-40②** 的落地判据）。
///
/// 口径（PRD §5.3.6.2 同步更新）：`file_tag.explicit` 区分两种行 ——
/// ① **显式行**（`explicit = 1`）：用户真的写了这个标签；② **祖先行**（`explicit = 0`）：MD-TAG-03 为层级展开补的。
/// - `include_children = false` ⇒ 只要 ①：**显式写了该标签**的笔记；
/// - `include_children = true`  ⇒ ① + 该标签**全部子孙**的笔记（祖先登记让子孙行自然落入，语义与生效前一致）。
///
/// 矩阵刻意覆盖三种笔记：**只写父标签** / **写子标签（父行是祖先登记）** / **父子都写** —— 缺任一种，
/// 这个开关"看起来生效"与"真的生效"就区分不开。
#[test]
fn include_children_switch_distinguishes_explicit_from_ancestors() {
    let (_v, state) = fixture(&[
        ("parent_only.md", "#项目 只写父\n"),
        ("child_only.md", "#项目/alpha 只写子\n"),
        ("both.md", "#项目 父子都写\n#项目/alpha 父子都写\n"),
        ("grandchild.md", "#项目/alpha/前端 写孙\n"),
    ]);

    // 父标签「项目」：只含自身 = 显式写过 #项目 的两篇（父子都写 + 只写父）
    assert_eq!(
        notes(&state, "项目", false),
        vec!["both.md", "parent_only.md"],
        "include_children=false ⇒ 只列【显式写了 #项目】的笔记（child_only 的 项目 行是祖先登记，不算）"
    );
    // 含全部子孙 = 四篇都算（child_only / grandchild 通过子孙标签落入）
    assert_eq!(
        notes(&state, "项目", true),
        vec![
            "both.md",
            "child_only.md",
            "grandchild.md",
            "parent_only.md"
        ],
        "include_children=true ⇒ 自身 + 全部子孙（与生效前的语义一致）"
    );

    // 中间层「项目/alpha」：显式写过它的两篇 vs 再加孙辈那篇
    assert_eq!(
        notes(&state, "项目/alpha", false),
        vec!["both.md", "child_only.md"]
    );
    assert_eq!(
        notes(&state, "项目/alpha", true),
        vec!["both.md", "child_only.md", "grandchild.md"]
    );

    // 叶子：两种取值相同（没有子孙可含）
    assert_eq!(
        notes(&state, "项目/alpha/前端", false),
        vec!["grandchild.md"]
    );
    assert_eq!(
        notes(&state, "项目/alpha/前端", true),
        vec!["grandchild.md"]
    );
}

/// **`file_tag.source` / `explicit` 的数据面判据**（批次 4 的 schema 变更）：
/// 正文标签写 `body`、frontmatter 标签写 `frontmatter`；只有**最末级**那一行 `explicit = 1`。
#[test]
fn file_tag_rows_carry_source_and_explicit() {
    let (_v, state) = fixture(&[
        ("fm.md", "---\ntags: [项目/甲]\n---\n正文无标签\n"),
        ("body.md", "#项目/乙 正文\n"),
    ]);
    let pool = state.index_db().expect("索引库");
    // 连标签名一起取出来：只比 (路径, 来源, explicit) 的话，同一篇的两行谁是祖先行要靠顺序去猜。
    let rows: Vec<(String, String, String, i64)> = pool
        .with_reader(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT f.rel_path, t.norm, ft.source, ft.explicit FROM file_tag ft \
                     JOIN file f ON f.id = ft.file_id JOIN tag t ON t.id = ft.tag_id \
                     WHERE t.norm IN ('项目', '项目/甲', '项目/乙') ORDER BY f.rel_path, t.norm",
                )
                .map_err(|e| kp_domain::error::AppError::db_with("准备读 file_tag", &e))?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(|e| kp_domain::error::AppError::db_with("读 file_tag", &e))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| kp_domain::error::AppError::db_with("收 file_tag", &e))
        })
        .expect("查询 file_tag");
    let owned =
        |p: &str, n: &str, s: &str, e: i64| (p.to_string(), n.to_string(), s.to_string(), e);
    assert_eq!(
        rows,
        vec![
            owned("body.md", "项目", "body", 0),
            owned("body.md", "项目/乙", "body", 1),
            owned("fm.md", "项目", "frontmatter", 0),
            owned("fm.md", "项目/甲", "frontmatter", 1),
        ],
        "祖先行的 explicit=0、显式行=1；source 区分 body/frontmatter（`项目` 两行都是祖先登记）"
    );
}

/// 边界：不存在的标签 ⇒ 空列表（不是报错）；未分类查询在"每篇都有标签"时 ⇒ 空列表。
#[test]
fn unknown_tag_returns_empty_and_unclassified_can_be_zero() {
    let (_v, state) = fixture(&[("a.md", "#有标签 甲\n")]);
    assert!(notes(&state, "不存在的标签", false).is_empty());
    assert!(
        notes(&state, "", false).is_empty(),
        "每篇都有标签 ⇒ 未分类为 0 条"
    );
    let tree = tag_tree_impl(&state).expect("标签树");
    assert_eq!(tree.last().expect("非空").ref_count, 0, "未分类计数为 0");
}
