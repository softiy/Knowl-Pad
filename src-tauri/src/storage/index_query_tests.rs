//! 文件树的索引数据源测试（M3 PR-6 / **AC-FILE-06** 的后端度量）。

use std::time::Instant;

use rusqlite::params;

use crate::storage::index::open;
use crate::storage::index_query::{has_rows, list_children};
use kp_domain::error::AppError;

fn seed(pool: &crate::storage::pool::DbPool, rel_path: &str, kind: &str) {
    // 写连接在工作线程上执行，闭包必须 move 且捕获 owned 数据
    let rel = rel_path.to_string();
    let kind = kind.to_string();
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path).to_string();
    pool.with_writer(move |conn| {
        conn.execute(
            "INSERT INTO file (rel_path, name, stem, ext, kind, size_bytes, mtime_ms, content_hash, deleted, indexed_at) \
             VALUES (?1, ?2, ?2, 'md', ?3, 1, 1, 'h', 0, 1)",
            params![rel, name, kind],
        )
        .map(|_| ())
        .map_err(|_| AppError::db("灌文件行"))
    })
    .expect("灌文件行");
}

#[test]
fn tree_from_index_groups_directories_and_files() {
    let dir = tempfile::tempdir().expect("临时目录");
    let pool = open(dir.path()).expect("建索引库");
    assert!(!has_rows(&pool), "空库应报告没有数据（调用方据此回退直读）");
    seed(&pool, "根笔记.md", "note");
    seed(&pool, "子目录/甲.md", "note");
    seed(&pool, "子目录/更深/乙.md", "note");
    seed(&pool, "图片.png", "attachment");
    assert!(has_rows(&pool));

    let root = list_children(&pool, "").expect("列举根");
    let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
    // 目录在前；其余按小写名称比较（中文按码点：图 U+56FE < 根 U+6839）
    assert_eq!(
        names,
        vec!["子目录", "图片.png", "根笔记.md"],
        "目录在前，其余按名称"
    );
    assert!(root[0].is_dir && root[0].has_children == Some(true));
    let png = root
        .iter()
        .find(|e| e.name == "图片.png")
        .expect("图片.png 应在列");
    assert_eq!(png.kind, kp_domain::file_tree::FileKind::Attachment);

    let sub = list_children(&pool, "子目录").expect("列举子目录");
    let sub_names: Vec<&str> = sub.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(sub_names, vec!["更深", "甲.md"], "更深一层应折叠为目录节点");
}

/// **AC-FILE-06（后端侧真机度量）**：1 万文件下根目录与子目录的列举耗时。
#[test]
fn ac_file_06_listing_scales_to_10k_files() {
    let dir = tempfile::tempdir().expect("临时目录");
    let pool = open(dir.path()).expect("建索引库");
    pool.with_writer(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE").map_err(|_| AppError::db("灌数据"))?;
        for i in 0..50i64 {
            conn.execute(
                "INSERT INTO file (rel_path, name, stem, ext, kind, size_bytes, mtime_ms, content_hash, deleted, indexed_at) \
                 VALUES (?1, ?2, ?2, 'md', 'note', 1, 1, 'h', 0, 1)",
                params![format!("根{i}.md"), format!("根{i}.md")],
            )
            .map_err(|_| AppError::db("灌根文件"))?;
        }
        for d in 0..100i64 {
            for f in 0..100i64 {
                conn.execute(
                    "INSERT INTO file (rel_path, name, stem, ext, kind, size_bytes, mtime_ms, content_hash, deleted, indexed_at) \
                     VALUES (?1, ?2, ?2, 'md', 'note', 1, 1, 'h', 0, 1)",
                    params![format!("目录{d}/笔记{f}.md"), format!("笔记{f}.md")],
                )
                .map_err(|_| AppError::db("灌子文件"))?;
            }
        }
        conn.execute_batch("COMMIT").map_err(|_| AppError::db("提交灌数"))?;
        Ok(())
    })
    .expect("灌入 1 万文件");

    let t0 = Instant::now();
    let root = list_children(&pool, "").expect("列举根");
    let root_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let sub = list_children(&pool, "目录7").expect("列举子目录");
    let sub_ms = t1.elapsed().as_millis();
    println!(
        "AC-FILE-06（后端）：10050 个文件下 → 根目录列举 {root_ms} ms（{} 项，150 个根项应为 50 文件 + 100 目录）、子目录 {sub_ms} ms（{} 项）",
        root.len(),
        sub.len()
    );
    assert_eq!(root.len(), 150, "根目录 = 50 个文件 + 100 个目录节点");
    assert_eq!(sub.len(), 100);
    assert!(root_ms < 2000, "根目录列举 {root_ms} ms 过慢");
    assert!(sub_ms < 2000, "子目录列举 {sub_ms} ms 过慢");
}
