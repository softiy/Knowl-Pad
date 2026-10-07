//! 块 ID 提取（MD-BID-01 ~ MD-BID-03）。
//!
//! - **MD-BID-01**：段落/列表项/表格行**末尾**的 `^标识符`，标识符为 `[a-zA-Z0-9-]+`；
//! - **MD-BID-02**：在其所属块内唯一（跨笔记可重复，锚点解析始终限定单笔记）；
//! - **MD-BID-03**：必须入库，供 `[[笔记#^块ID]]` 锚点跳转。

use std::ops::Range;

use super::code_fence::CodeRanges;

/// 一个块 ID 及其所属块的行范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockId {
    pub id: String,
    /// 1-based：块 ID 所在行
    pub line: u32,
    /// 1-based：所属块的起始行
    pub start_line: u32,
    /// 1-based：所属块的结束行
    pub end_line: u32,
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// 提取块 ID（按出现顺序）。
pub fn extract(text: &str, code: &CodeRanges, skip: Option<Range<usize>>) -> Vec<BlockId> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut offsets = Vec::with_capacity(lines.len());
    let mut acc = 0usize;
    for line in &lines {
        offsets.push(acc);
        acc += line.len();
    }
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let start = offsets[idx];
        if code.contains(start) || skip.as_ref().is_some_and(|r| r.contains(&start)) {
            continue;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']).trim_end();
        // 末尾的 ^id（前面必须是空白或行首）
        let Some(caret) = trimmed.rfind('^') else {
            continue;
        };
        let id = &trimmed[caret + 1..];
        if id.is_empty() || !id.chars().all(is_id_char) {
            continue;
        }
        let before_ok = caret == 0
            || trimmed[..caret]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());
        if !before_ok {
            continue;
        }
        // 所属块：从本行向上回溯连续非空行（跳过代码/frontmatter 的空洞即停）
        let mut block_start = idx;
        let mut probe = idx;
        while probe > 0 {
            let prev_start = offsets[probe - 1];
            if code.contains(prev_start) || skip.as_ref().is_some_and(|r| r.contains(&prev_start)) {
                break;
            }
            let prev = lines[probe - 1].trim_end_matches(['\n', '\r']);
            if prev.trim().is_empty() {
                break;
            }
            block_start = probe - 1;
            probe -= 1;
        }
        out.push(BlockId {
            id: id.to_string(),
            line: idx as u32 + 1,
            start_line: block_start as u32 + 1,
            end_line: idx as u32 + 1,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(text: &str) -> Vec<BlockId> {
        extract(text, &CodeRanges::scan(text), None)
    }

    #[test]
    fn b18_block_id_at_paragraph_end() {
        let b = ids("第一段第一行\n第一段第二行 ^blockid\n\n第二段\n");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].id, "blockid");
        assert_eq!((b[0].start_line, b[0].end_line), (1, 2), "块范围覆盖整段");
    }

    #[test]
    fn bid01_allows_digits_and_hyphen() {
        let b = ids("文字 ^a-b-123\n");
        assert_eq!(b[0].id, "a-b-123");
    }

    #[test]
    fn bid01_rejects_invalid_identifier_chars() {
        assert!(
            ids("文字 ^bad_id\n").is_empty(),
            "下划线不在 [a-zA-Z0-9-] 内"
        );
        assert!(ids("文字 ^\n").is_empty(), "空标识符");
        assert!(ids("文字x^id\n").is_empty(), "^ 前必须是空白或行首");
    }

    #[test]
    fn block_id_inside_code_is_ignored() {
        assert!(ids("```\n^codeid\n```\n").is_empty());
    }
}
