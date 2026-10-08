//! 反链上下文片段（FR-LINK-11 / FR-LINK-12）——**纯函数**，面板与测试共用。
//!
//! 约定（与 `md_parse::wikilink` 一致，见 `wikilink.rs` 的 `line_col`）：
//! - `line` **1 基**；
//! - `col` **1 基的字符偏移**（Unicode scalar，不是字节）；
//! - 返回的高亮区间相对**返回的那一行文本**，0 基、左闭右开，便于前端直接切片高亮。

/// FR-LINK-11：上下文"前后各 N 行"的默认值。
pub const DEFAULT_CONTEXT_LINES: usize = 2;

// 注：本模块**不带 serde**（kp-domain 不依赖 serde，见技术方案的分层约定）——
// IPC 形状由 `src-tauri` 的命令层负责映射与序列化。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetLine {
    /// 1 基行号
    pub line: u32,
    /// 该行原文（已去掉行尾 \r）
    pub text: String,
    /// 是否是链接所在行
    pub is_link_line: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub lines: Vec<SnippetLine>,
    /// 链接所在行（1 基）
    pub highlight_line: u32,
    /// 高亮起点（相对高亮行文本，0 基，字符）
    pub highlight_start: usize,
    /// 高亮终点（相对高亮行文本，0 基，左闭右开，字符）
    pub highlight_end: usize,
}

/// 取 `line`（1 基）前后各 `before`/`after` 行的上下文片段，并给出链接的高亮区间。
///
/// `col` 为 1 基字符列；`link_len_chars` 为链接自身的字符数（用于算高亮终点）。
/// 行号越界（0 或超过总行数）返回 `None` —— 索引与磁盘可能短暂不一致，调用方据此降级。
pub fn extract_snippet(
    content: &str,
    line: u32,
    col: u32,
    link_len_chars: usize,
    before: usize,
    after: usize,
) -> Option<Snippet> {
    if line == 0 {
        return None;
    }
    // 统一按 \n 切分，并去掉行尾 \r（CRLF 文稿也要能对齐行号）
    let all: Vec<&str> = content
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // 空内容视为"没有行"：`"".split('\n')` 会产出一个空串，那不是可展示的第 1 行。
    let total = if content.is_empty() { 0 } else { all.len() };
    let idx = (line - 1) as usize;
    if idx >= total {
        return None;
    }
    let start = idx.saturating_sub(before);
    let end = (idx + after + 1).min(total);
    let lines: Vec<SnippetLine> = (start..end)
        .map(|i| SnippetLine {
            line: (i + 1) as u32,
            text: all[i].to_string(),
            is_link_line: i == idx,
        })
        .collect();
    // col 是 1 基字符列；防御式处理 col=0 与越界
    let line_chars = all[idx].chars().count();
    let highlight_start = (col.max(1) as usize - 1).min(line_chars);
    let highlight_end = (highlight_start + link_len_chars).min(line_chars);
    Some(Snippet {
        lines,
        highlight_line: line,
        highlight_start,
        highlight_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_at(s: &Snippet, line: u32) -> String {
        s.lines
            .iter()
            .find(|l| l.line == line)
            .map(|l| l.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn takes_before_and_after_lines_with_1_based_numbers() {
        let doc = "一\n二\n三\n四\n五";
        let s = extract_snippet(doc, 3, 1, 1, 1, 1).expect("第 3 行应可提取");
        assert_eq!(s.lines.len(), 3);
        assert_eq!(s.lines[0].line, 2);
        assert_eq!(text_at(&s, 3), "三");
        assert_eq!(s.highlight_line, 3);
        assert_eq!(s.lines.iter().filter(|l| l.is_link_line).count(), 1);
        assert!(s.lines[1].is_link_line, "中间那行才该是链接行");
    }

    #[test]
    fn clamps_at_file_start_and_end() {
        let doc = "一\n二\n三";
        let head = extract_snippet(doc, 1, 1, 1, 2, 2).expect("首行");
        assert_eq!(head.lines.first().unwrap().line, 1, "不得出现 0 或负行号");
        assert_eq!(head.lines.len(), 3);
        let tail = extract_snippet(doc, 3, 1, 1, 2, 2).expect("末行");
        assert_eq!(tail.lines.last().unwrap().line, 3);
        assert_eq!(tail.lines.len(), 3);
    }

    #[test]
    fn zero_context_returns_only_the_link_line() {
        let doc = "一\n二\n三";
        let s = extract_snippet(doc, 2, 1, 1, 0, 0).expect("仅本行");
        assert_eq!(s.lines.len(), 1);
        assert_eq!(s.lines[0].line, 2);
    }

    #[test]
    fn out_of_range_lines_return_none() {
        let doc = "一\n二";
        assert!(extract_snippet(doc, 0, 1, 1, 2, 2).is_none(), "0 行不存在");
        assert!(extract_snippet(doc, 3, 1, 1, 2, 2).is_none(), "超出末行");
        assert!(
            extract_snippet("", 1, 1, 1, 2, 2).is_none(),
            "空内容没有第 1 行"
        );
    }

    #[test]
    fn crlf_line_endings_do_not_leak_carriage_returns() {
        let doc = "一\r\n二\r\n三\r\n";
        let s = extract_snippet(doc, 2, 1, 1, 1, 1).expect("CRLF 文稿");
        for l in &s.lines {
            assert!(!l.text.contains('\r'), "行文本不得带 \\r：{:?}", l.text);
        }
        assert_eq!(text_at(&s, 2), "二");
    }

    /// 关键：col 是**字符**偏移而不是字节 —— 中文行必须按字符算。
    #[test]
    fn highlight_uses_character_offsets_not_bytes() {
        let doc = "见 [[B]] 与其它";
        // 「见 」= 2 个字符 → 链接从第 3 个字符开始（1 基 col = 3）
        let s = extract_snippet(doc, 1, 3, 5, 0, 0).expect("中文行");
        let chars: Vec<char> = doc.chars().collect();
        let highlighted: String = chars[s.highlight_start..s.highlight_end].iter().collect();
        assert_eq!(highlighted, "[[B]]", "高亮区间必须正好覆盖链接（字符偏移）");
    }

    #[test]
    fn highlight_is_clamped_when_beyond_line_end() {
        let doc = "短";
        let s = extract_snippet(doc, 1, 9, 5, 0, 0).expect("越界列应被夹紧");
        assert!(s.highlight_start <= 1 && s.highlight_end <= 1);
        assert!(s.highlight_start <= s.highlight_end);
    }

    #[test]
    fn default_context_is_two_lines_per_side() {
        assert_eq!(DEFAULT_CONTEXT_LINES, 2, "FR-LINK-11 的默认 N");
    }
}
