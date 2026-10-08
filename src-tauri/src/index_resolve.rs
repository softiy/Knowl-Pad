//! 链接裁决与标签计数重算（勘误 D-20：裁决属 **M3 索引阶段**，不是 M4）。
//!
//! 候选来源（技术方案 §5.3.4 的三步）：`stem` → `rel_path`（含/不含 `.md`）→ `file_alias.alias`，
//! 均**大小写不敏感**。
//!
//! 判定：候选恰好 1 个 → `resolved` 并写入 `dst_file_id`；0 个 → `dangling`；
//! ≥2 个时按 **MD-WL-04** 消歧：**同目录优先 → 最短路径优先**；仍不唯一 → `ambiguous`（不猜）。
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

/// 候选：(文件 id, 相对路径)。**路径是 MD-WL-04 消歧所必需的**。
type Candidate = (i64, String);

/// 小写键 → 候选列表。
type CandidateMap = HashMap<String, Vec<Candidate>>;

fn push(map: &mut CandidateMap, key: String, id: i64, rel_path: &str) {
    let entry = map.entry(key).or_default();
    if !entry.iter().any(|(existing, _)| *existing == id) {
        entry.push((id, rel_path.to_string()));
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
            push(&mut by_name, stem.to_lowercase(), id, &rel_path);
            let rel_lower = rel_path.to_lowercase();
            push(&mut by_name, rel_lower.clone(), id, &rel_path);
            if let Some(stripped) = rel_lower.strip_suffix(".md") {
                push(&mut by_name, stripped.to_string(), id, &rel_path);
            }
        }
    }
    {
        let mut stmt = conn
            .prepare(
                "SELECT fa.file_id, fa.alias, f.rel_path FROM file_alias fa \
                 JOIN file f ON f.id = fa.file_id WHERE f.deleted = 0",
            )
            .map_err(|_| AppError::db("读取别名表"))?;
        let mut rows = stmt.query([]).map_err(|_| AppError::db("读取别名表"))?;
        while let Some(row) = rows.next().map_err(|_| AppError::db("读取别名表"))? {
            let id: i64 = row.get(0).map_err(|_| AppError::db("读取别名表"))?;
            let alias: String = row.get(1).map_err(|_| AppError::db("读取别名表"))?;
            let rel_path: String = row.get(2).map_err(|_| AppError::db("读取别名表"))?;
            push(&mut by_alias, alias.to_lowercase(), id, &rel_path);
        }
    }
    Ok((by_name, by_alias))
}

/// 对全部链接行重新裁决（**幂等**：可重复调用）。
pub fn resolve_links(conn: &Connection) -> Result<ResolveOutcome, AppError> {
    let (by_name, by_alias) = build_candidates(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT l.id, l.target_ref, f.rel_path FROM link l \
             JOIN file f ON f.id = l.src_file_id",
        )
        .map_err(|_| AppError::db("读取待裁决链接"))?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|_| AppError::db("读取待裁决链接"))?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut out = ResolveOutcome::default();
    for (link_id, target, src_rel_path) in rows {
        let (status, dst) = judge(&by_name, &by_alias, &target, &src_rel_path);
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

/// **增量裁决**：只重算受影响的链接，而不是全表。
///
/// 受影响 = ① 这些文件自己的出链（`src_file_id ∈ changed`）；
/// ② 目标键命中这些文件的**其他**链接（`target_ref` 等于其 stem/rel_path/别名 —— 含"该文件刚被删除/改名"的情形，
/// 因此目标键必须由调用方**在写库之前**采集）。
///
/// 候选表仍然是 O(N) 建一次（10 万文件实测约百毫秒级），真正贵的是逐条 UPDATE，故这里靠收敛链接数取胜。
pub fn resolve_links_matching(
    conn: &Connection,
    changed_file_ids: &[i64],
    target_keys: &[String],
) -> Result<ResolveOutcome, AppError> {
    if changed_file_ids.is_empty() && target_keys.is_empty() {
        return Ok(ResolveOutcome::default());
    }
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _kp_changed(id INTEGER PRIMARY KEY); \
         DELETE FROM _kp_changed; \
         CREATE TEMP TABLE IF NOT EXISTS _kp_targets(key TEXT PRIMARY KEY); \
         DELETE FROM _kp_targets;",
    )
    .map_err(|_| AppError::db("建临时变更表"))?;
    for id in changed_file_ids {
        conn.execute(
            "INSERT OR IGNORE INTO _kp_changed(id) VALUES (?1)",
            rusqlite::params![id],
        )
        .map_err(|_| AppError::db("写入变更文件"))?;
    }
    for key in target_keys {
        conn.execute(
            "INSERT OR IGNORE INTO _kp_targets(key) VALUES (?1)",
            rusqlite::params![key.to_lowercase()],
        )
        .map_err(|_| AppError::db("写入变更目标键"))?;
    }
    let (by_name, by_alias) = build_candidates(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT l.id, l.target_ref, f.rel_path FROM link l \
             JOIN file f ON f.id = l.src_file_id \
              WHERE l.src_file_id IN (SELECT id FROM _kp_changed) \
                 OR lower(l.target_ref) IN (SELECT key FROM _kp_targets)",
        )
        .map_err(|_| AppError::db("读取受影响链接"))?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|_| AppError::db("读取受影响链接"))?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);
    let mut out = ResolveOutcome::default();
    for (link_id, target, src_rel_path) in rows {
        let (status, dst) = judge(&by_name, &by_alias, &target, &src_rel_path);
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

/// 取相对路径的目录部分（无 `/` 表示 Vault 根）。
fn dir_of(rel_path: &str) -> &str {
    rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// **MD-WL-04 消歧**：① 与源文件**同目录**的候选唯一 → 选它；
/// ② 否则（或同目录有多个）在候选集里取**路径最短**者（按路径段数）；
/// ③ 仍不唯一 → `None`（保持 `ambiguous`，不猜）。
fn disambiguate(candidates: &[Candidate], src_rel_path: &str) -> Option<i64> {
    let src_dir = dir_of(src_rel_path);
    let same_dir: Vec<&Candidate> = candidates
        .iter()
        .filter(|(_, path)| dir_of(path) == src_dir)
        .collect();
    if same_dir.len() == 1 {
        return Some(same_dir[0].0);
    }
    let pool: Vec<&Candidate> = if same_dir.is_empty() {
        candidates.iter().collect()
    } else {
        same_dir
    };
    // "最短路径"取**层数**（路径段数），不拿字符长度当决胜 —— 那会在等深的候选之间
    // 凭一两个字符的偶然差异替用户做选择，违背"宁可 ambiguous 也不猜"。
    let depth = |p: &str| p.matches('/').count();
    let best = pool.iter().map(|(_, p)| depth(p)).min()?;
    let mut winners = pool.iter().filter(|(_, p)| depth(p) == best);
    let first = winners.next()?;
    if winners.next().is_some() {
        return None; // 等深并列 → 不猜
    }
    Some(first.0)
}

/// 裁决单条：1 个候选 → resolved；0 → dangling；
/// ≥2 → 先按 MD-WL-04 消歧（同目录 → 最短路径），仍不唯一才 ambiguous（不猜）。
fn judge(
    by_name: &CandidateMap,
    by_alias: &CandidateMap,
    target: &str,
    src_rel_path: &str,
) -> (&'static str, Option<i64>) {
    let key = target.trim().to_lowercase();
    if key.is_empty() {
        return ("dangling", None);
    }
    let without_md = key.strip_suffix(".md").unwrap_or(&key).to_string();
    let mut candidates: Vec<Candidate> = Vec::new();
    for map_key in [&key, &without_md] {
        if let Some(found) = by_name.get(map_key) {
            for (id, path) in found {
                if !candidates.iter().any(|(existing, _)| existing == id) {
                    candidates.push((*id, path.clone()));
                }
            }
        }
    }
    if candidates.is_empty() {
        if let Some(found) = by_alias.get(&key) {
            candidates.extend(found.iter().cloned());
        }
    }
    match candidates.len() {
        1 => ("resolved", Some(candidates[0].0)),
        0 => ("dangling", None),
        _ => match disambiguate(&candidates, src_rel_path) {
            Some(id) => ("resolved", Some(id)),
            None => ("ambiguous", None),
        },
    }
}
