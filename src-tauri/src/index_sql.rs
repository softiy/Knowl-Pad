//! 索引写入路径里**走预编译语句缓存**（`prepare_cached`）的全部 SQL。
//!
//! 为什么单列一个模块：rusqlite 的语句缓存有**个数上限**（默认 16，见 rusqlite
//! `lib.rs` 的 `STATEMENT_CACHE_DEFAULT_CAPACITY`），超出后按 LRU 淘汰 —— 淘汰本身
//! **不影响正确性**（只是丢掉一次缓存命中、退回到"每次重新 prepare"），但我们仍然要能回答
//! "我们会不会把它挤爆"。把全部缓存语句集中在这里，就能用一个用例把这件事钉住
//! （见下方 `cached_statement_count_fits_default_capacity`）。
//!
//! **SQL 文本一字未改**：与加缓存之前逐字一致，这里只做"从调用点提成常量"这一件事。
//! 语义不变由 `AC-REL-03`（重建后逐表逐字节一致）与 `tag_write_semantics_*` 兜住。

/// 旧 file 行 id（note_fts 的 rowid 与 file.id 对齐，必须在删 file 之前取）。
pub const OLD_FILE_ID_SQL: &str = "SELECT id FROM file WHERE rel_path = ?1";
/// 删旧 FTS 行。
pub const FTS_DELETE_SQL: &str = "DELETE FROM note_fts WHERE rowid = ?1";
/// 删旧 file 行。
pub const FILE_DELETE_SQL: &str = "DELETE FROM file WHERE id = ?1";
/// 写 file 行。
pub const FILE_INSERT_SQL: &str =
    "INSERT INTO file (rel_path, name, stem, ext, kind, size_bytes, mtime_ms, content_hash, deleted, indexed_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)";
/// 写 note_fts 行。
pub const FTS_INSERT_SQL: &str = "INSERT INTO note_fts(rowid, plain_text) VALUES (?1, ?2)";
/// 写 heading 行。
pub const HEADING_INSERT_SQL: &str =
    "INSERT INTO heading (file_id, level, text, anchor, line, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";
/// 写 block_id 行。
pub const BLOCK_ID_INSERT_SQL: &str =
    "INSERT OR IGNORE INTO block_id (file_id, bid, line_start, line_end) VALUES (?1, ?2, ?3, ?4)";
/// 写 link 行。
pub const LINK_INSERT_SQL: &str =
    "INSERT INTO link (src_file_id, dst_file_id, target_ref, anchor, alias, link_kind, status, line, col) \
     VALUES (?1, NULL, ?2, ?3, ?4, ?5, 'dangling', ?6, ?7)";
/// 写 tag 行（upsert：**display 首次写入者胜、结构字段最后一次写入者胜**）。
///
/// **为什么要分开**（2026-10-16 裁定，见 PRD 勘误 D-40①）：
/// - `display`（显示名）由 **AC-TAG-03 / FR-TAG-09** 定死为「**首次**出现的原文形式」⇒ 冲突时**不动它**；
/// - `depth` / `is_leaf` 等**结构字段**必须反映**最终**层级（AC-TAG-01 的 `is_leaf` 仅最末级为真）
///   ⇒ 冲突时照旧最后一次写入者胜。
///
/// 语义钉子见 `tag_write_semantics_display_first_wins_structure_last_wins` 用例。
pub const TAG_UPSERT_SQL: &str =
    "INSERT INTO tag (norm, display, depth, is_leaf, ref_count) VALUES (?1, ?2, ?3, ?4, 0) \
     ON CONFLICT(norm) DO UPDATE SET depth = excluded.depth, is_leaf = excluded.is_leaf";
/// 取 tag id。
pub const TAG_SELECT_ID_SQL: &str = "SELECT id FROM tag WHERE norm = ?1";
/// 写 file_tag 行。
///
/// `source` / `explicit` 是 2026-10-17（M5 批次 4）新增的两列（PRD §5.3.6.1、勘误 **D-40②**）：
/// - `source` 供 **FR-TAG-10** 的「来源可区分」（`body` = 正文标签，`frontmatter` = frontmatter 的 `tags:`）；
/// - `explicit` = 1 表示这行是**用户显式写出**的标签，0 表示它是 MD-TAG-03 为层级展开补出来的祖先 ——
///   FR-TAG-03 的 `include_children = false` 只取 explicit = 1 的行（此前两列不存在，该开关是 no-op）。
pub const FILE_TAG_INSERT_SQL: &str =
    "INSERT INTO file_tag (file_id, tag_id, line, col, source, explicit) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";
/// 写 file_alias 行。
pub const ALIAS_INSERT_SQL: &str =
    "INSERT OR IGNORE INTO file_alias (file_id, alias) VALUES (?1, ?2)";

/// **全部走缓存的 SQL**（新增一条就必须同时进这个数组 —— 下面的容量守卫依赖它）。
///
/// 只在 test 构建里存在：它是"容量守卫"的输入，生产代码不需要这份清单。
#[cfg(test)]
pub const ALL_CACHED_SQL: [&str; 12] = [
    OLD_FILE_ID_SQL,
    FTS_DELETE_SQL,
    FILE_DELETE_SQL,
    FILE_INSERT_SQL,
    FTS_INSERT_SQL,
    HEADING_INSERT_SQL,
    BLOCK_ID_INSERT_SQL,
    LINK_INSERT_SQL,
    TAG_UPSERT_SQL,
    TAG_SELECT_ID_SQL,
    FILE_TAG_INSERT_SQL,
    ALIAS_INSERT_SQL,
];

/// 清理**孤儿标签行**：没有任何 `file_tag` 引用（`ref_count = 0`）的 `tag` 行。
///
/// **依据**（2026-10-16 裁定，见 PRD 勘误 **D-41**）：
/// - AC-TAG-02 末句要求「`#old` 若无其他子孙则从标签表中消失」；
/// - 重建结果必须与一次性索引**逐表一致**（AC-REL-03 的口径）—— 孤儿行会让两者不同，
///   属**正确性缺陷**而不是面板美观问题（它还会被 `tag_tree` 列出来，点进去是空列表）。
///
/// **只在全量重建收尾调用**：重建中途某标签可能暂时为 0（它的文件还没轮到），早删会把后面还要用的标签删掉。
pub const ORPHAN_TAGS_DELETE_SQL: &str = "DELETE FROM tag WHERE ref_count = 0 AND NOT EXISTS \
     (SELECT 1 FROM file_tag ft WHERE ft.tag_id = tag.id)";

#[cfg(test)]
mod tests {
    use super::*;

    /// rusqlite 的语句缓存默认容量（rusqlite-0.40.2/src/lib.rs 的
    /// `STATEMENT_CACHE_DEFAULT_CAPACITY`，实测值）。**改 rusqlite 版本时要复核这一条。**
    const RUSQLITE_DEFAULT_STATEMENT_CACHE_CAPACITY: usize = 16;

    /// 索引路径用到的缓存语句数必须**留在默认容量以内** —— 否则 LRU 会把正在用的语句淘汰掉，
    /// 每个语句都退回"每次重新 prepare"，优化静默失效（结果仍然正确，但白干）。
    #[test]
    fn cached_statement_count_fits_default_capacity() {
        assert!(
            ALL_CACHED_SQL.len() <= RUSQLITE_DEFAULT_STATEMENT_CACHE_CAPACITY,
            "缓存语句 {} 条 > rusqlite 默认容量 {}：要么减少缓存语句，要么显式调大 \
             Connection::set_prepared_statement_cache_capacity（并在此处登记）",
            ALL_CACHED_SQL.len(),
            RUSQLITE_DEFAULT_STATEMENT_CACHE_CAPACITY
        );
    }

    /// 缓存按 **SQL 文本**做键：两条语句若文本相同就会共用同一个条目（那是浪费），
    /// 若文本只差空白则是**两条**（各自占一格）。这里钉住"没有重复文本"。
    #[test]
    fn cached_statements_are_textually_distinct() {
        let mut seen: Vec<&str> = Vec::new();
        for sql in ALL_CACHED_SQL {
            let key = sql.trim();
            assert!(!seen.contains(&key), "缓存语句文本重复：{key}");
            seen.push(key);
        }
        assert_eq!(seen.len(), ALL_CACHED_SQL.len());
    }
}
