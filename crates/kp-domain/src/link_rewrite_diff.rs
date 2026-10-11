//! 改写预览的**逐处差异**（技术方案 §6.1.1 期望的"每一处的原文 → 新文"）。
//!
//! 纯函数：给定改写前后的全文，按行比对，找出**变化的那一段**。
//! 因为改写只替换链接文本、行的其余部分逐字不变，所以"最长公共前缀 + 最长公共后缀"
//! 裁出来的区间正好就是 `[[旧]]` → `[[新]]` 这一段（带锚点/别名时也只在段内变化）。

/// 一处变化（行号 1-based，与索引口径一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrenceDiff {
    pub line: u32,
    /// 变化前的片段（如 `[[A]]`）
    pub before: String,
    /// 变化后的片段（如 `[[B]]`）
    pub after: String,
}

/// 逐行比对，返回最多 `max` 处变化；行数不同的尾部按整行变化处理。
///
/// `max` 用于控制 IPC 载荷：预览只需要让人看清"改成什么"，不需要把上万处全传过去。
pub fn occurrence_diffs(old: &str, new: &str, max: usize) -> Vec<OccurrenceDiff> {
    if max == 0 {
        return Vec::new();
    }
    let old_lines: Vec<&str> = old.split('\n').collect();
    let new_lines: Vec<&str> = new.split('\n').collect();
    let mut out = Vec::new();
    let n = old_lines.len().max(new_lines.len());
    for i in 0..n {
        let a = old_lines.get(i).copied().unwrap_or("");
        let b = new_lines.get(i).copied().unwrap_or("");
        if a == b {
            continue;
        }
        let (before, after) = changed_span(a, b);
        out.push(OccurrenceDiff {
            line: (i + 1) as u32,
            before,
            after,
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// 裁出两行之间变化的片段：先取最小差异段，**再扩到包住整条链接**。
///
/// 为什么要扩：最小差异段只会是 \`A\` → \`B\`（\`[[\` 与 \`]]\` 是公共前后缀），
/// 而 §6.1.1 要的是"**每一处的原文 → 新文**"——让人一眼看到是**哪条链接**变成了什么，
/// 所以把区间扩到包含 \`[[\` 与 \`]]\`（别名/锚点一起带上）。找不到记号时保持最小段。
///
/// 正确性依据：公共前缀（\`pre\` 之前）与公共后缀（\`suf\` 之后）在两行里**逐字相同**，
/// 因此"在旧行里往前扩 k 个字符"与"在新行里往前扩 k 个字符"取到的是同一批字符，
/// 两侧可以用同一组绝对下标。
fn changed_span(a: &str, b: &str) -> (String, String) {
    let ab: Vec<char> = a.chars().collect();
    let bb: Vec<char> = b.chars().collect();
    let mut pre = 0;
    while pre < ab.len() && pre < bb.len() && ab[pre] == bb[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < ab.len().saturating_sub(pre)
        && suf < bb.len().saturating_sub(pre)
        && ab[ab.len() - 1 - suf] == bb[bb.len() - 1 - suf]
    {
        suf += 1;
    }
    let a_mid_start = pre;
    let b_mid_start = pre;
    let a_mid_end = ab.len() - suf;
    let b_mid_end = bb.len() - suf;

    // 向前扩到最近的 "[["（只在公共前缀里找；找不到就不扩）
    let mut start = a_mid_start;
    while start >= 2 && !(ab[start - 1] == '[' && ab[start - 2] == '[') {
        start -= 1;
    }
    let widened_start = start >= 2;
    if widened_start {
        start -= 2;
    } else {
        start = a_mid_start;
    }

    // 向后扩到最近的 "]]"（只在公共后缀里找）
    let mut grow = 0;
    if widened_start {
        let mut i = a_mid_end;
        while i + 1 < ab.len() && !(ab[i] == ']' && ab[i + 1] == ']') {
            i += 1;
        }
        if i + 1 < ab.len() && ab[i] == ']' && ab[i + 1] == ']' {
            grow = i + 2 - a_mid_end;
        }
    }

    let before: String = ab[start..a_mid_end + grow].iter().collect();
    let after: String = bb[start..b_mid_end + grow].iter().collect();
    // 兜底：万一两侧长度不一致（理论上不会），退回最小差异段
    if a_mid_start < start {
        let min_before: String = ab[a_mid_start..a_mid_end].iter().collect();
        let min_after: String = bb[b_mid_start..b_mid_end].iter().collect();
        if before.chars().count() != min_before.chars().count() + (a_mid_start - start) {
            return (min_before, min_after);
        }
    }
    (before, after)
}
