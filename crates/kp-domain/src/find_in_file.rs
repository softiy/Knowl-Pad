//! 文件内查找与替换的纯逻辑（FR-SEARCH-30/31；选项语义复用 FR-EDITOR-09）。
//!
//! **为什么只做字面量查找**：本批的硬约束是零新依赖（红线 R-16/R-17）。FR-EDITOR-09 的
//! 「正则」选项需要 regex crate —— 技术方案 §3.5.2 的矩阵把它列为 M3 引入项，但 M3 未实际引入
//! （crates/kp-domain/Cargo.toml 里没有它）。依赖未落地之前**不提供**一个会静默降级成正则的开关：
//! 宁可少一个选项，也不要一个「看起来支持正则」的假实现（登记见本 PR 报告的口径缺口）。
//!
//! 大小写不敏感采用 **ASCII 折叠**（eq_ignore_ascii_case）：与 FTS5 unicode61 对 ASCII 的行为一致；
//! 非 ASCII（含中文）按**码点精确**比较 —— 中文无大小写，而全量 Unicode 折叠会改变字符数
//! （如 'İ' 折叠成两个码点），使字节偏移与字符列不再可信。

use crate::error::AppError;

/// 查找选项（PRD §5.3.4 的 options；语义出处 FR-EDITOR-09）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FindOptions {
    /// 区分大小写；默认**不区分**（编辑器查找的常见默认，且与 FTS5 索引侧行为一致）
    pub case_sensitive: bool,
    /// 全字匹配
    pub whole_word: bool,
}

/// 一处命中：字节区间 + 1 基行号/字符列（行列口径与 md_parse、snippet 一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSite {
    /// 原文中的字节区间（左闭右开）——按它精确落刀，避免按行列二次解析
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: u32,
    /// 1 基**字符**列（中文按 1 个字符算，不是字节）
    pub col: u32,
    /// 命中的字符数（前端据此算高亮终点；字节数不等于显示宽度）
    pub len_chars: usize,
}

/// 替换目标：字节区间 + **期望原文**（用于陈旧检测，R-07 不许静默覆盖）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceTarget {
    pub start_byte: usize,
    pub end_byte: usize,
    pub expect: String,
}

/// 全部行首的字节偏移（含首行的 0），供字节偏移 → 行列映射。
fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    starts.extend(
        content
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(i, _)| i + 1),
    );
    starts
}

/// 把一个已知的字节区间映射为 MatchSite（区间必须落在字符边界上）。
pub fn site_at(content: &str, start_byte: usize, end_byte: usize) -> Option<MatchSite> {
    site_with_starts(content, &line_starts(content), start_byte, end_byte)
}

fn site_with_starts(
    content: &str,
    starts: &[usize],
    start_byte: usize,
    end_byte: usize,
) -> Option<MatchSite> {
    if start_byte >= end_byte
        || end_byte > content.len()
        || !content.is_char_boundary(start_byte)
        || !content.is_char_boundary(end_byte)
    {
        return None;
    }
    // starts[0] == 0，故 partition_point - 1 恒不为负
    let line_idx = starts.partition_point(|b| *b <= start_byte) - 1;
    let line_start = starts[line_idx];
    Some(MatchSite {
        start_byte,
        end_byte,
        line: (line_idx + 1) as u32,
        col: content[line_start..start_byte].chars().count() as u32 + 1,
        len_chars: content[start_byte..end_byte].chars().count(),
    })
}

/// 取某一行的文本（不含行尾的 \r\n / \n）。
fn line_text_at(content: &str, starts: &[usize], line: u32) -> Option<String> {
    let idx = line.checked_sub(1)? as usize;
    let start = *starts.get(idx)?;
    // 下一行的行首减 1 即本行的 \n 位置；末行取到文件末尾
    let end = starts
        .get(idx + 1)
        .map(|next| next.saturating_sub(1))
        .unwrap_or(content.len());
    let raw = content.get(start..end.max(start))?;
    Some(raw.strip_suffix('\r').unwrap_or(raw).to_string())
}

/// 命中 + 所在整行文本（FR-SEARCH-30 的「第 X / 共 Y 处」要连上下文一起展示）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedMatch {
    pub site: MatchSite,
    pub line_text: String,
}

/// 查找全部命中并附上整行文本。**行首表只建一次** —— 逐个命中各扫一遍全文在长笔记上会退化。
pub fn find_all_with_lines(content: &str, query: &str, opts: FindOptions) -> Vec<LocatedMatch> {
    let starts = line_starts(content);
    scan(content, query, opts)
        .into_iter()
        .filter_map(|(s, e)| {
            let site = site_with_starts(content, &starts, s, e)?;
            let line_text = line_text_at(content, &starts, site.line)?;
            Some(LocatedMatch { site, line_text })
        })
        .collect()
}

/// 词字符（全字匹配用）：字母/数字/下划线。汉字属 alphabetic，因此「图谱」里的「图」不算独立词。
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn eq_at(hay: &[u8], at: usize, needle: &[u8], case_sensitive: bool) -> bool {
    needle.iter().enumerate().all(|(k, b)| {
        let h = hay[at + k];
        if case_sensitive {
            h == *b
        } else {
            h.eq_ignore_ascii_case(b)
        }
    })
}

fn word_ok(content: &str, start: usize, end: usize, whole_word: bool) -> bool {
    if !whole_word {
        return true;
    }
    let before = content[..start].chars().next_back();
    let after = content[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

/// 字节级字面量扫描：返回升序、**互不重叠**的字节区间（编辑器的「查找全部」口径）。
///
/// 直接在 UTF-8 字节上比较是安全的：合法 UTF-8 的首字节不可能是续字节，
/// 因此命中起点必然落在字符边界上，而完整匹配的字节序列必然覆盖完整的码点。
fn scan(content: &str, query: &str, opts: FindOptions) -> Vec<(usize, usize)> {
    let hay = content.as_bytes();
    let needle = query.as_bytes();
    let mut out = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() {
        return out;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if eq_at(hay, i, needle, opts.case_sensitive)
            && word_ok(content, i, i + needle.len(), opts.whole_word)
        {
            out.push((i, i + needle.len()));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// 在 content 中找出 query 的全部命中（空查询返回空表，而不是「全部命中」）。
pub fn find_all(content: &str, query: &str, opts: FindOptions) -> Vec<MatchSite> {
    scan(content, query, opts)
        .into_iter()
        .filter_map(|(s, e)| site_at(content, s, e))
        .collect()
}

/// 应用替换：**先逐条校验期望原文**，再自后向前落刀（保证前面的偏移不失效）。
///
/// 返回 Err 表示「有目标与当前内容对不上」——文件在查找之后被改过、目标区间重叠或越界。
/// 调用方据此返回 E_WRITE_CONFLICT 并**不做任何写入**：按旧偏移硬改就是静默破坏用户数据（P-2 / R-07）。
pub fn apply_replacements(
    content: &str,
    targets: &[ReplaceTarget],
    replacement: &str,
) -> Result<String, AppError> {
    if targets.is_empty() {
        return Ok(content.to_string());
    }
    let mut sorted: Vec<&ReplaceTarget> = targets.iter().collect();
    sorted.sort_by_key(|t| t.start_byte);
    let mut prev_end = 0usize;
    for t in sorted.iter() {
        let intact = t.start_byte < t.end_byte
            && t.start_byte >= prev_end
            && t.end_byte <= content.len()
            && content.is_char_boundary(t.start_byte)
            && content.is_char_boundary(t.end_byte)
            && content.get(t.start_byte..t.end_byte) == Some(t.expect.as_str());
        if !intact {
            return Err(AppError::WriteConflict(
                "目标处的原文与查找结果不一致（文件可能已被外部修改）".to_string(),
            ));
        }
        prev_end = t.end_byte;
    }
    let mut out = content.to_string();
    for t in sorted.iter().rev() {
        out.replace_range(t.start_byte..t.end_byte, replacement);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "find_in_file_tests.rs"]
mod tests;
