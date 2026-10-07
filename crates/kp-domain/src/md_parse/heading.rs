//! 标题识别（MD-H-01 ~ MD-H-03）。
//!
//! - **MD-H-01**：ATX（`#`~`######` 后跟空格）与 Setext（`===`/`---` 下划线）都要识别；代码块内的 `#` 不是标题；
//! - **MD-H-02**：标题文本去除首尾空白与行内 Markdown 标记（粗体/斜体/行内代码/删除线）后入库，用于锚点匹配；
//! - **MD-H-03**：同一笔记内标题文本重复时，锚点匹配取**第一个**，后续标记 `duplicate`（供 `heading_ambiguous`）。

use std::ops::Range;

use super::code_fence::CodeRanges;

/// 一个标题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    /// 已剥离行内标记的文本（MD-H-02）
    pub text: String,
    pub line: u32,
    /// 是否与前面的标题重名（MD-H-03）
    pub duplicate: bool,
}

/// 剥离行内 Markdown 标记（MD-H-02 的近似实现：成对/单独的分隔符一律移除）。
fn strip_markers(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' => {
                // 连续同类分隔符一并跳过（** __ `` ~~）
                while chars.peek() == Some(&c) {
                    chars.next();
                }
            }
            '~' => {
                let mut n = 1;
                while chars.peek() == Some(&'~') {
                    chars.next();
                    n += 1;
                }
                if n < 2 {
                    out.push('~');
                }
            }
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// 提取标题（按出现顺序）。
pub fn extract(text: &str, code: &CodeRanges, skip: Option<Range<usize>>) -> Vec<Heading> {
    let mut out: Vec<Heading> = Vec::new();
    let mut offset = 0usize;
    let mut prev_text: Option<(String, usize)> = None; // (上一行原文, 行起始字节)
    for (idx, line) in text.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let skipped =
            code.contains(line_start) || skip.as_ref().is_some_and(|r| r.contains(&line_start));
        if skipped {
            prev_text = None;
            continue;
        }
        // Setext：本行全部为 = 或 -，且上一行是非空段落文本
        let is_setext = !trimmed.trim().is_empty()
            && (trimmed.trim().chars().all(|c| c == '=')
                || trimmed.trim().chars().all(|c| c == '-'));
        if is_setext {
            if let Some((prev, _)) = prev_text.take() {
                let level = if trimmed.trim().starts_with('=') {
                    1
                } else {
                    2
                };
                let text = strip_markers(&prev);
                if !text.is_empty() {
                    let duplicate = out.iter().any(|h| h.text == text);
                    out.push(Heading {
                        level,
                        text,
                        line: idx as u32,
                        duplicate,
                    });
                }
            }
            continue;
        }
        let t = trimmed.trim_start();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        let is_atx = (1..=6).contains(&hashes)
            && t.chars()
                .nth(hashes)
                .map(|c| c == ' ' || c == '\t')
                .unwrap_or(t.len() == hashes);
        if is_atx {
            let rest = t.chars().skip(hashes).collect::<String>();
            let text = strip_markers(rest.trim().trim_end_matches('#'));
            if !text.is_empty() {
                let duplicate = out.iter().any(|h| h.text == text);
                out.push(Heading {
                    level: hashes as u8,
                    text,
                    line: idx as u32 + 1,
                    duplicate,
                });
            }
            prev_text = None;
            continue;
        }
        prev_text = if trimmed.trim().is_empty() {
            None
        } else {
            Some((trimmed.to_string(), line_start))
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(text: &str) -> Vec<Heading> {
        extract(text, &CodeRanges::scan(text), None)
    }

    #[test]
    fn h01_atx_levels() {
        let h = heads("# 一级\n### 三级\n####### 七个不是标题\n");
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].level, h[0].text.as_str()), (1, "一级"));
        assert_eq!((h[1].level, h[1].text.as_str()), (3, "三级"));
    }

    #[test]
    fn b21_setext_heading_is_level_one() {
        let h = heads("标题\n===\n正文\n");
        assert_eq!(h.len(), 1);
        // 行号约定：**标题文本所在行**（Setext 的下划线行不算）
        assert_eq!((h[0].level, h[0].text.as_str(), h[0].line), (1, "标题", 1));
    }

    #[test]
    fn h01_hash_inside_code_is_not_heading() {
        assert!(heads("```\n# 代码里的井号\n```\n").is_empty());
    }

    #[test]
    fn h02_inline_markers_are_stripped() {
        let h = heads("# **粗** 与 *斜* 与 `码` 与 ~~删~~\n");
        assert_eq!(h[0].text, "粗 与 斜 与 码 与 删");
    }

    #[test]
    fn h03_duplicate_is_flagged() {
        let h = heads("# 同名\n# 同名\n");
        assert_eq!(h.len(), 2);
        assert!(!h[0].duplicate);
        assert!(h[1].duplicate, "MD-H-03：重复标题需标记");
    }
}
