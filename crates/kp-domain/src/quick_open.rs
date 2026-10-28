//! 快速打开的**模糊匹配打分**（FR-SEARCH-40/41）。
//!
//! **为什么后端自己算**：PRD §5.3.4 把 search_quick_open 定义为一个**命令**（入参 query/limit），
//! 而 FR-SEARCH-40 点名的 fuse.js 按技术方案 §3.2.5 是**纯前端、不走 IPC** 的库。
//! 后端命令必须能独立给出候选与排序（前端仍可用 fuse.js 做二次排序或调权），
//! 因此这里实现一个**零依赖、确定性**的子序列打分器（红线 R-16：不为此引入新依赖）。
//!
//! 打分口径（可解释、可复现，不是调参出来的）：
//! - 子序列匹配、大小写不敏感（ASCII 折叠）：work/rep 能命中 work/2026/report.md（FR-SEARCH-41）；
//! - 起点越靠前越好、跳跃越少越好、连续匹配越多越好、覆盖字符越多越好；
//! - 命中落在**文件名部分**（最后一个 / 之后）额外加分 —— 用户敲的通常是文件名。
//!
//! 这里**不**解析 / 的分段语义（不要求「目录片段整段匹配」）：FR-SEARCH-41 只要求路径片段能命中。

/// 连续匹配的加分（每一对相邻命中）。
const CONTIG_BONUS: i32 = 8;
/// 覆盖字符的加分（每个命中字符）。
const COVER_BONUS: i32 = 1;
/// 起点靠后的扣分上限（避免一个超长路径被扣成负数而排到「不匹配」的观感）。
const START_PENALTY_CAP: i32 = 20;
/// 每个跳跃字符的扣分与总扣分上限。
const GAP_PENALTY: i32 = 2;
const GAP_PENALTY_CAP: i32 = 30;
/// 命中落在文件名部分的加分。
const BASENAME_BONUS: i32 = 20;

/// 单字符比较：ASCII 折叠（非 ASCII 按码点精确比较，理由见 find_in_file 的模块注释）。
fn eq_char(a: char, b: char) -> bool {
    if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// 对单个候选文本打分；**不匹配返回 None**。空查询返回 Some(0)（全部候选同分）。
pub fn score(text: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    // **ASCII 快路径**（实测占快速打开候选的绝大多数）：按字节扫描，避免逐字符解码；
    // ASCII 下字节下标 == 字符下标，打分公式与通用路径完全一致。
    if query.is_ascii() && text.is_ascii() {
        return score_ascii(text, query);
    }
    let needle: Vec<char> = query.chars().collect();
    // 不分配 hay 的 Vec<char>（10 万候选 × 每次分配是原实现的主要开销）；
    // 用 chars().enumerate() 的字符下标，与原实现的语义完全一致（start/gaps/basename 都按字符计）。
    let mut iter = text.chars().enumerate();
    let mut first: Option<usize> = None;
    let mut prev: Option<usize> = None;
    let mut gaps = 0usize;
    let mut runs = 0usize;
    for want in &needle {
        let mut found = None;
        for (idx, ch) in iter.by_ref() {
            if eq_char(ch, *want) {
                found = Some(idx);
                break;
            }
        }
        let pos = found?;
        match prev {
            None => {
                first = Some(pos);
                runs = 1;
            }
            Some(p) if pos == p + 1 => {}
            Some(p) => {
                gaps += pos - p - 1;
                runs += 1;
            }
        }
        prev = Some(pos);
    }
    let matched = needle.len() as i32;
    let contiguous = matched - runs as i32;
    let start = first.unwrap_or(0) as i32;
    // 文件名部分的起始字符下标（无 / 时为 0，即整条路径都算文件名）
    let basename_start = text
        .rfind('/')
        .map(|b| text[..b].chars().count() + 1)
        .unwrap_or(0) as i32;
    let in_basename = i32::from(start >= basename_start);
    Some(
        COVER_BONUS * matched + CONTIG_BONUS * contiguous + BASENAME_BONUS * in_basename
            - start.min(START_PENALTY_CAP)
            - (gaps as i32 * GAP_PENALTY).min(GAP_PENALTY_CAP),
    )
}

fn score_ascii(text: &str, query: &str) -> Option<i32> {
    let hay = text.as_bytes();
    let needle = query.as_bytes();
    let mut cursor = 0usize;
    let mut first: Option<usize> = None;
    let mut prev: Option<usize> = None;
    let mut gaps = 0usize;
    let mut runs = 0usize;
    for want in needle {
        let want = want.to_ascii_lowercase();
        let mut found = None;
        while cursor < hay.len() {
            if hay[cursor].to_ascii_lowercase() == want {
                found = Some(cursor);
                break;
            }
            cursor += 1;
        }
        let pos = found?;
        match prev {
            None => {
                first = Some(pos);
                runs = 1;
            }
            Some(p) if pos == p + 1 => {}
            Some(p) => {
                gaps += pos - p - 1;
                runs += 1;
            }
        }
        prev = Some(pos);
        cursor = pos + 1;
    }
    let matched = needle.len() as i32;
    let contiguous = matched - runs as i32;
    let start = first.unwrap_or(0) as i32;
    let basename_start = text.rfind('/').map(|b| b + 1).unwrap_or(0) as i32;
    let in_basename = i32::from(start >= basename_start);
    Some(
        COVER_BONUS * matched + CONTIG_BONUS * contiguous + BASENAME_BONUS * in_basename
            - start.min(START_PENALTY_CAP)
            - (gaps as i32 * GAP_PENALTY).min(GAP_PENALTY_CAP),
    )
}

/// 排序：分数降序 → 原顺序（稳定，保证同分时结果可复现）。最多返回 limit 条。
///
/// limit 为 0 时返回空表（调用方必须先把它夹到合法区间；这里不再"顺手"给默认值，
/// 因为静默的默认值会让 IPC 契约的边界变得不可预测）。
pub fn rank(candidates: &[String], query: &str, limit: usize) -> Vec<(usize, i32)> {
    if limit == 0 {
        return Vec::new();
    }
    // **预筛**（必要条件的超集）：子序列匹配要求候选的**字符数** ≥ 查询字符数；
    // 而字符数 ≤ 字节数，所以 `text.len() < needle_len` 一定不可能匹配 —— 直接跳过，语义不变。
    let needle_len = query.chars().count();
    let mut scored: Vec<(usize, i32)> = Vec::with_capacity(candidates.len().min(limit * 8));
    for (i, text) in candidates.iter().enumerate() {
        if needle_len > 0 && text.len() < needle_len {
            continue;
        }
        if let Some(score) = score(text, query) {
            scored.push((i, score));
        }
    }
    // 全序 = 分数降序、同分按原下标升序（与旧实现的稳定排序逐项等价）。
    let key = |a: &(usize, i32), b: &(usize, i32)| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0));
    // **top-k 部分选择**：只取前 limit 个，避免对全部命中做完整排序。
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit - 1, key);
        scored.truncate(limit);
    }
    scored.sort_by(key);
    scored
}

/// 旧实现（参考用，仅测试）：全部分数 + 稳定排序 + 截断。
#[cfg(test)]
pub fn rank_reference(candidates: &[String], query: &str, limit: usize) -> Vec<(usize, i32)> {
    if limit == 0 {
        return Vec::new();
    }
    let mut scored: Vec<(usize, i32)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, text)| score(text, query).map(|s| (i, s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn path_fragment_matches_nested_path() {
        // FR-SEARCH-41：输入 work/rep 命中 work/2026/report.md
        assert!(score("work/2026/report.md", "work/rep").is_some());
        assert!(score("notes/readme.md", "work/rep").is_none());
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(score("Work/Report.md", "work/rep").is_some());
        let a = score("report.md", "REP");
        let b = score("report.md", "rep");
        assert_eq!(a, b, "大小写不影响得分");
    }

    #[test]
    fn basename_match_beats_directory_match() {
        let ranked = rank(&list(&["rep/其他.md", "工作/report.md"]), "rep", 5);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].0, 1, "命中文件名的那条应排在前面");
    }

    #[test]
    fn contiguous_match_beats_scattered_match() {
        let ranked = rank(&list(&["r-e-p.md", "report.md"]), "rep", 5);
        assert_eq!(ranked[0].0, 1, "连续命中 report.md 应排在 r-e-p.md 之前");
    }

    #[test]
    fn closer_to_the_start_ranks_higher() {
        let ranked = rank(&list(&["长长长长前缀/report.md", "report.md"]), "report", 5);
        assert_eq!(ranked[0].0, 1);
    }

    #[test]
    fn empty_query_matches_everything_with_equal_score() {
        let candidates = list(&["甲.md", "乙.md", "丙.md"]);
        let ranked = rank(&candidates, "", 2);
        assert_eq!(ranked.len(), 2, "limit 生效");
        assert_eq!(ranked[0], (0, 0));
        assert_eq!(ranked[1], (1, 0), "同分保持原有顺序（路径升序）");
    }

    #[test]
    fn limit_zero_returns_nothing() {
        assert!(rank(&list(&["甲.md"]), "甲", 0).is_empty());
    }

    #[test]
    fn no_candidate_matches_yields_empty() {
        assert!(rank(&list(&["甲.md", "乙.md"]), "zzz", 5).is_empty());
    }

    #[test]
    fn single_char_query_beats_nothing_but_prefers_prefix() {
        let ranked = rank(&list(&["a/丙.md", "甲.md"]), "a", 5);
        assert_eq!(ranked.len(), 1, "只有含 a 的候选命中");
        assert_eq!(ranked[0].0, 0);
    }

    /// **等价性**：优化后的 rank 与旧实现（参考）在 30 个 query 上必须逐项相同（含顺序）。
    ///
    /// 反向验证：把预筛条件收紧一个字符（len < needle_len + 1）⇒ 本用例必红
    /// （候选 note 与查询 note 等长，会被错误剪掉）。
    #[test]
    fn optimized_rank_matches_reference_for_thirty_queries() {
        let mut candidates: Vec<String> = Vec::new();
        for i in 0..300 {
            candidates.push(format!("work/{}/Report-{i}.md", i % 7));
            candidates.push(format!("notes/readme-{i}.md"));
            candidates.push(format!("Work/archive/{i}/REPORT.md"));
        }
        candidates.push("note".to_string());
        candidates.push("Note".to_string());
        let queries = [
            "note",
            "NOTE",
            "rep",
            "work/rep",
            "wr",
            "readme",
            "md",
            "re7",
            "w",
            "a",
            "arch",
            "Report",
            "report",
            "R",
            "no",
            "notes/",
            "archive",
            "x",
            "zzz",
            "work/0",
            "work/00",
            "wrr",
            "n0",
            "REP0",
            "work/report",
            "e",
            "m",
            "d",
            "0",
            "Work/Arch",
        ];
        for query in queries {
            let got = rank(&candidates, query, 20);
            let want = rank_reference(&candidates, query, 20);
            assert_eq!(got, want, "query={query} 的新旧 top-k 必须逐项一致");
        }
    }
}
