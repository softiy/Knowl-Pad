//! **AC-VAULT-01 的解析断言**（D-25 接收清单第 1 条；PRD §4.1.2 AC-VAULT-01）。
//!
//! PRD 原文（Given/When/Then）：300 篇笔记、wikilink 与嵌套标签、存在第三方配置目录（`.obsidian/`）；
//! 「索引完成且不报错；配置目录内文件数量与内容与打开前**完全一致**（逐字节）；文件树显示全部 300 篇笔记；
//! 随机抽查 20 篇，其链接与标签解析结果**符合 §3.1 对应规则条款的期望值**」。
//!
//! 本用例的**边界**（如实登记）：存储/路径部分 M1 已验收；这里只做解析断言，且走**同一条** `open` + `full_index_cancellable`
//! 调用链（不含 Tauri 的 IPC 薄壳与窗口/文件树前端 —— 那两跳由 §11.6 的真机验收覆盖）。
//! 期望值是**按 PRD §3.1 规则手工推导后固化在断言里**的（路径形态 wikilink ⇒ resolved；不存在的目标 ⇒ dangling；
//! `![[assets/img-N.png]]` ⇒ resolved 到附件；`#a/b/c` ⇒ 深度 2 的叶子标签；frontmatter `tags:` ⇒ 也进标签表），
//! 不以任何外部软件的运行时行为为基准（AC 原文要求）。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::index_engine::{full_index_cancellable, IndexMode};
use crate::storage::index::open;
use crate::storage::link_query::outgoing_for;
use crate::storage::pool::DbPool;
use crate::storage::tag_query::files_with_tag;
use kp_domain::tag_tree::TagRow;

const NOTES: usize = 300;
/// 抽查篇数（PRD 原文：随机抽查 20 篇）。这里取**确定性**的前 20 篇（可复现比"随机"更重要）。
const CHECKED: usize = 20;

/** 造一篇笔记：路径链接 + 标题 + 悬空链接 + 嵌套标签 + frontmatter 标签 + 附件嵌入。 */
fn write_note(root: &Path, i: usize) {
    let rel = format!("d{}/note-{i}.md", i % 10);
    let p = root.join(&rel);
    fs::create_dir_all(p.parent().expect("父目录")).expect("建目录");
    let next_dir = (i + 1) % 10;
    let next_note = (i + 1) % NOTES;
    let body = format!(
        "---\ntags: [fm/区{}, fm/层级]\n---\n\n# 标题 {i}\n\n\
         [[d{next_dir}/note-{next_note}]] 与 #标签/{}/{}\n\
         悬空：[[缺失的目标-{i}]]\n\n![[assets/img-{}.png]]\n",
        i % 5,
        i % 7,
        i % 3,
        i % 4
    );
    fs::write(&p, body).expect("写笔记");
}

/** 造第三方配置目录（含普通 JSON 与一段"二进制样"内容，用来证明逐字节不变）。 */
fn write_obsidian(root: &Path) {
    let dir = root.join(".obsidian");
    fs::create_dir_all(dir.join("plugins/p1")).expect("建 .obsidian");
    fs::write(dir.join("app.json"), b"{\n  \"x\": 1\n}\n").expect("写 app.json");
    fs::write(dir.join("plugins/p1/data.json"), b"{\"k\":\"v\"}").expect("写 data.json");
    let bytes: Vec<u8> = (0u8..=255).collect();
    fs::write(dir.join("plugins/p1/blob.bin"), &bytes).expect("写 blob.bin");
}

/** 递归读取目录下的 (相对路径 → 字节) 快照，用于逐字节比对。 */
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).expect("读目录") {
            let entry = entry.expect("目录项");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .expect("相对路径")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).expect("读文件"));
            }
        }
    }
    out
}

/** 造夹具：300 篇笔记 + 4 个附件 + `.obsidian/`。返回临时目录守卫与根路径。 */
fn build_vault() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    for i in 0..NOTES {
        write_note(&root, i);
    }
    fs::create_dir_all(root.join("assets")).expect("建 assets");
    for k in 0..4 {
        fs::write(
            root.join(format!("assets/img-{k}.png")),
            b"\x89PNG\r\n\x1a\n",
        )
        .expect("写附件");
    }
    write_obsidian(&root);
    (dir, root)
}

/// 索引库（临时目录，不写进夹具 —— 否则 `.obsidian/` 的"逐字节一致"断言会被我们自己的库干扰）。
fn index_pool() -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("索引库临时目录");
    let pool = open(&dir.path().join("index.db")).expect("打开索引库");
    (dir, pool)
}

/** 从库里数笔记数（不靠夹具自报）。 */
fn note_count(pool: &DbPool) -> i64 {
    pool.with_reader(|c| {
        c.query_row(
            "SELECT count(*) FROM file WHERE deleted = 0 AND kind = 'note'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| kp_domain::error::AppError::db_with("数笔记", &e))
    })
    .expect("数笔记")
}

#[test]
fn d25_ac_vault_01_parse() {
    let (guard, root) = build_vault();
    let before_obsidian = snapshot(&root.join(".obsidian"));
    let (db_guard, pool) = index_pool();

    let outcome = full_index_cancellable(&pool, &root, |_, _| {}, IndexMode::Force, || false)
        .expect("打开并索引应成功");
    assert!(
        outcome.warnings.is_empty(),
        "索引完成且不报错（AC 原文）：warnings={:?}",
        outcome.warnings
    );
    assert_eq!(
        note_count(&pool),
        NOTES as i64,
        "文件树/索引应显示全部 {NOTES} 篇"
    );

    // ② 第三方配置目录逐字节不变（文件数 + 每个文件的字节）
    let after_obsidian = snapshot(&root.join(".obsidian"));
    assert_eq!(
        after_obsidian.len(),
        before_obsidian.len(),
        ".obsidian 文件数不得变化"
    );
    assert_eq!(
        after_obsidian, before_obsidian,
        ".obsidian 内容必须逐字节一致"
    );

    // ③ 抽查 20 篇：链接（路径 resolved / 悬空 dangling / 附件 resolved）与标签（嵌套 + frontmatter）
    for i in 0..CHECKED {
        let rel = format!("d{}/note-{i}.md", i % 10);
        let rows = outgoing_for(&pool, &rel).expect("读取出链");
        assert_eq!(
            rows.len(),
            3,
            "{rel} 应有 3 条出链（1 路径链接 + 1 悬空链接 + 1 附件嵌入）；实际 {rows:?}"
        );
        let resolved = rows
            .iter()
            .find(|r| r.link_kind == "link" && r.status == "resolved")
            .unwrap_or_else(|| panic!("{rel} 必须有一条 resolved 的路径链接：{rows:?}"));
        let expected = format!("d{}/note-{}.md", (i + 1) % 10, (i + 1) % NOTES);
        assert_eq!(
            resolved.dst_rel_path.as_deref(),
            Some(expected.as_str()),
            "{rel} 的路径链接应解析到 {expected}"
        );
        let dangling = rows
            .iter()
            .find(|r| r.status == "dangling")
            .unwrap_or_else(|| panic!("{rel} 必须有一条 dangling：{rows:?}"));
        assert_eq!(dangling.target_ref, format!("缺失的目标-{i}"));
        assert!(dangling.dst_rel_path.is_none(), "悬空链接不得有目标路径");
        // 附件嵌入（`![[assets/img-N.png]]`）也必须解析到附件（PRD §3.1 的 embed 规则）
        let embed = rows
            .iter()
            .find(|r| r.link_kind == "embed")
            .unwrap_or_else(|| panic!("{rel} 必须有一条 embed：{rows:?}"));
        let expected_asset = format!("assets/img-{}.png", i % 4);
        assert_eq!(embed.status, "resolved", "附件嵌入应解析成功");
        assert_eq!(embed.dst_rel_path.as_deref(), Some(expected_asset.as_str()));

        // 嵌套标签：`#标签/{i%7}/{i%3}` ⇒ 该标签必须收录本篇（且是叶子标签）
        let tag = format!("标签/{}/{}", i % 7, i % 3);
        let files = files_with_tag(&pool, &tag).expect("按标签取文件");
        assert!(
            files.contains(&rel),
            "{rel} 必须被收录在标签 #{tag} 下：{files:?}"
        );
    }

    // ④ 标签表：嵌套深度与叶子标记（`#标签/a/b` ⇒ 标签 / 标签/a / 标签/a/b 三层）
    let rows: Vec<TagRow> = crate::storage::tag_query::load_tag_rows(&pool).expect("读标签表");
    let find = |norm: &str| rows.iter().find(|r| r.norm == norm).cloned();
    let root_tag = find("标签").expect("应存在根标签 `标签`");
    let mid_tag = find("标签/0").expect("应存在 `标签/0`");
    let leaf_tag = find("标签/0/0").expect("应存在 `标签/0/0`");
    // `depth` 是**段数**（1 = 根标签，2 = `a/b`，3 = `a/b/c`），不是 0 基层级
    assert_eq!(root_tag.depth, 1, "`标签` 的 depth 应是 1");
    assert_eq!(mid_tag.depth, 2, "`标签/0` 的 depth 应是 2");
    assert_eq!(leaf_tag.depth, 3, "`标签/0/0` 的 depth 应是 3");
    assert!(leaf_tag.is_leaf, "`标签/0/0` 应是叶子标签");
    assert!(!root_tag.is_leaf, "有子标签的节点不得标成叶子");
    // frontmatter 的 `tags: [fm/区N, fm/层级]` 也必须进标签表（PRD §3.1 的 frontmatter 规则）
    assert!(find("fm/层级").is_some(), "frontmatter 标签必须进标签表");

    let _ = (&guard, &db_guard);
    println!(
        "D25 AC-VAULT-01-parse ok=1 notes={} checked={} indexed={} warnings={} obsidian_files={} tags={}",
        NOTES,
        CHECKED,
        outcome.indexed,
        outcome.warnings.len(),
        after_obsidian.len(),
        rows.len()
    );
}
