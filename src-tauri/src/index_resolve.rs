//! 链接裁决与标签计数重算（勘误 D-20：裁决属 **M3 索引阶段**，不是 M4）。
//!
//! 候选来源（技术方案 §5.3.4 的三步）：`stem` → `rel_path`（含/不含 `.md`）→ `file_alias.alias`，
//! 均**大小写不敏感**。
//!
//! 判定：候选恰好 1 个 → `resolved` 并写入 `dst_file_id`；0 个 → `dangling`；≥2 个 → `ambiguous`。
//! **本切片不自动消歧**（MD-WL-04 的自动消解规则待补）——宁可标 ambiguous 让人看见，也不猜错。
//!
//! **性能（DEBT-04 实测逼出来的实现）**：候选表**一次性建好**（O(N)），随后每条链接 O(1) 查表。
//! 早期实现是「每条链接一条 SQL」，在 10 万文件 / 1 万链接下实测 **750 秒**（`lower(stem) = lower(?1)`
//! 用不上 `idx_file_stem_lower` 表达式索引 → 每条链接全表扫 10 万行 ≈ 10 亿次比较）。
//! 改为查表后同一规模降到亚秒级。

use std::collections::HashMap;

use rusqlite::Connection;

use kp_domain::error::AppError;

/// 裁决结果统计。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolveOutcome {
    pub resolved: usize,
    pub dangling: usize,
    pub ambiguous: usize,
}

/// 小写键 → 候选文件 id 列表。
type CandidateMap = HashMap<String, Vec<i64>>;

fn push(map: &mut CandidateMap, key: String, id: i64) {
    let entry = map.entry(key).or_default();
    if !entry.contains(&id) {
        entry.push(id);
    }
}

/// 一次性建立候选表：stem / rel_path / rel_path 去 .md / 别名（全部小写）。
fn build_candidates(conn: &Connection) -> Result<(CandidateMap, CandidateMap), AppError> {
    let mut by_name: CandidateMap = HashMap::new();
    let mut by_alias: CandidateMap = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, stem, rel_path FROM file WHERE deleted = 0")
            .map_err(|_| AppError::db("读取文件表"))?;
        let mut rows = stmt.query([]).map_err(|_| AppError::db("读取文件表"))?;
        while let Some(row) = rows.next().map_err(|_| AppError::db("读取文件表"))? {
            let id: i64 = row.get(0).map_err(|_| AppError::db("读取文件表"))?;
            let stem: String = row.get(1).map_err(|_| AppError::db("读取文件表"))?;
            let rel_path: String = row.get(2).map_err(|_| AppError::db("读取文件表"))?;
            push(&mut by_name, stem.to_lowercase(), id);
            let rel_lower = rel_path.to_lowercase();
            push(&mut by_name, rel_lower.clone(), id);
            if let Some(stripped) = rel_lower.strip_suffix(".md") {
                push(&mut by_name, stripped.to_string(), id);
            }
        }
    }
    {
        let mut stmt = conn
            .prepare(
                "SELECT fa.file_id, fa.alias FROM file_alias fa JOIN file f ON f.id = fa.file_id WHERE f.deleted = 0",
            )
            .map_err(|_| AppError::db("读取别名表"))?;
        let mut rows = stmt.query([]).map_err(|_| AppError::db("读取别名表"))?;
        while let Some(row) = rows.next().map_err(|_| AppError::db("读取别名表"))? {
            let id: i64 = row.get(0).map_err(|_| AppError::db("读取别名表"))?;
            let alias: String = row.get(1).map_err(|_| AppError::db("读取别名表"))?;
            push(&mut by_alias, alias.to_lowercase(), id);
        }
    }
    Ok((by_name, by_alias))
}

/// 对全部链接行重新裁决（**幂等**：可重复调用）。
pub fn resolve_links(conn: &Connection) -> Result<ResolveOutcome, AppError> {
    let (by_name, by_alias) = build_candidates(conn)?;
    let mut stmt = conn
        .prepare("SELECT id, target_ref FROM link")
        .map_err(|_| AppError::db("读取待裁决链接"))?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| AppError::db("读取待裁决链接"))?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut out = ResolveOutcome::default();
    for (link_id, target) in rows {
        let key = target.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        let without_md = key.strip_suffix(".md").unwrap_or(&key).to_string();
        let mut candidates: Vec<i64> = Vec::new();
        for map_key in [&key, &without_md] {
            if let Some(ids) = by_name.get(map_key) {
                for id in ids {
                    if !candidates.contains(id) {
                        candidates.push(*id);
                    }
                }
            }
        }
        if candidates.is_empty() {
            if let Some(ids) = by_alias.get(&key) {
                candidates.extend(ids.iter().copied());
            }
        }
        let (status, dst) = match candidates.len() {
            1 => ("resolved", Some(candidates[0])),
            0 => ("dangling", None),
            _ => ("ambiguous", None),
        };
        conn.execute(
            "UPDATE link SET status = ?1, dst_file_id = ?2 WHERE id = ?3",
            rusqlite::params![status, dst, link_id],
        )
        .map_err(|_| AppError::db("写回裁决结果"))?;
        match status {
            "resolved" => out.resolved += 1,
            "ambiguous" => out.ambiguous += 1,
            _ => out.dangling += 1,
        }
    }
    Ok(out)
}

/// 重算标签 `ref_count`（= 引用该标签的文件-标签行数）。
pub fn recount_tags(conn: &Connection) -> Result<usize, AppError> {
    let n = conn
        .execute(
            "UPDATE tag SET ref_count = (SELECT count(*) FROM file_tag WHERE file_tag.tag_id = tag.id)",
            [],
        )
        .map_err(|_| AppError::db("重算标签引用计数"))?;
    Ok(n)
}
