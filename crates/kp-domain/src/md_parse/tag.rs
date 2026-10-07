//! 标签提取（MD-TAG-01 ~ MD-TAG-06）。
//!
//! - **MD-TAG-01**：字符集 = 中日韩文字/字母/数字/下划线/连字符/斜杠；**不得以数字开头**；
//! - **MD-TAG-02**：终止于空白、行尾或标点；
//! - **MD-TAG-03**：层级标签同时登记自身与全部祖先（`#a/b/c` → `a/b/c`、`a/b`、`a`），`is_leaf` 仅最末级为真；
//! - **MD-TAG-04**：代码块/行内代码/frontmatter 之外，`#` 后紧跟空格者为**标题**而非标签；
//! - **MD-TAG-05**：存储保留原文大小写，`norm` 为小写（匹配与去重按 norm）；
//! - **MD-TAG-06**：frontmatter `tags:` 同样登记，`line = -1`（无行列号）。

use std::ops::Range;

use super::code_fence::CodeRanges;

/// 一个标签条目（层级已展开）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// 原文大小写（MD-TAG-05）
    pub name: String,
    /// 规范化键（小写）
    pub norm: String,
    /// 1-based 行号；**-1 表示来自 frontmatter**（MD-TAG-06）
    pub line: i64,
    /// 1-based 列号（frontmatter 来源为 0）
    pub col: u32,
    /// 是否最末级（MD-TAG-03）
    pub is_leaf: bool,
}

/// 标签允许的字符（MD-TAG-01）。
fn is_tag_char(c: char) -> bool {
    !c.is_whitespace() && (c.is_alphanumeric() || c == '_' || c == '-' || c == '/')
}

/// 标签的起始边界：行首、空白，或常见标点（避免把 `abc#def` 当标签）。
fn is_start_boundary(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '(' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '[' | '{'
        )
}

/// 去掉首尾 `/` 并折叠空段（`#/a//b/` → `a/b`）。
fn clean(raw: &str) -> String {
    raw.split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// 展开层级（MD-TAG-03）：`a/b/c` → [a/b/c(leaf), a/b, a]。
fn expand(name: &str, line: i64, col: u32, out: &mut Vec<Tag>) {
    let parts: Vec<&str> = name.split('/').collect();
    for i in (1..=parts.len()).rev() {
        let joined = parts[..i].join("/");
        out.push(Tag {
            norm: joined.to_lowercase(),
            name: joined,
            line,
            col,
            is_leaf: i == parts.len(),
        });
    }
}

/// 正文中的标签（正文扫描）。
pub fn extract(text: &str, code: &CodeRanges, skip: Option<Range<usize>>) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for (line_idx, line) in text.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let chars: Vec<(usize, char)> = trimmed.char_indices().collect();
        // MD-TAG-04：行首（允许缩进）的 `#`+空格 = ATX 标题，本行的这一处不当标签
        let atx_hash = {
            let mut it = chars.iter().skip_while(|(_, c)| *c == ' ' || *c == '\t');
            match it.next() {
                Some((i, '#')) => {
                    let hashes = chars
                        .iter()
                        .skip_while(|(j, _)| *j < *i)
                        .take_while(|(_, c)| *c == '#')
                        .count();
                    let after = chars.iter().find(|(j, _)| *j == i + hashes);
                    Some((*i, matches!(after, Some((_, c)) if *c == ' ' || *c == '\t')))
                }
                _ => None,
            }
        };
        for (idx, (byte_in_line, ch)) in chars.iter().enumerate() {
            if *ch != '#' {
                continue;
            }
            let abs = line_start + byte_in_line;
            if code.contains(abs) || skip.as_ref().is_some_and(|r| r.contains(&abs)) {
                continue;
            }
            if let Some((h, is_atx)) = atx_hash {
                if is_atx && h == *byte_in_line {
                    continue;
                }
            }
            // 起始边界：行首或前一字符为空白/标点
            let ok_start = if idx == 0 {
                true
            } else {
                is_start_boundary(chars[idx - 1].1)
            };
            if !ok_start {
                continue;
            }
            let raw: String = chars[idx + 1..]
                .iter()
                .map(|(_, c)| *c)
                .take_while(|c| is_tag_char(*c))
                .collect();
            let name = clean(&raw);
            if name.is_empty() {
                continue;
            }
            // MD-TAG-01：不得以数字开头
            if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                continue;
            }
            let col = trimmed[..*byte_in_line].chars().count() as u32 + 1;
            expand(&name, line_idx as i64 + 1, col, &mut out);
        }
    }
    out
}

/// frontmatter 里的标签（MD-TAG-06：`line = -1`，无行列号）。
pub fn from_frontmatter(values: &[String]) -> Vec<Tag> {
    let mut out = Vec::new();
    for v in values {
        let name = clean(v.trim().trim_start_matches('#'));
        if name.is_empty() || name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        expand(&name, -1, 0, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(text: &str) -> Vec<Tag> {
        extract(text, &CodeRanges::scan(text), None)
    }

    #[test]
    fn b11_simple_tag() {
        let t = tags("正文 #标签 结束");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name, "标签");
        assert!(t[0].is_leaf);
        assert_eq!(t[0].line, 1);
    }

    #[test]
    fn b12_hierarchy_expands_to_ancestors() {
        let t = tags("#父/子/孙");
        let names: Vec<&str> = t.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["父/子/孙", "父/子", "父"],
            "MD-TAG-03：自身 + 全部祖先"
        );
        assert_eq!(
            t.iter().filter(|x| x.is_leaf).count(),
            1,
            "只有最末级 is_leaf"
        );
    }

    #[test]
    fn b13_numeric_tag_is_rejected() {
        assert!(tags("#123").is_empty(), "MD-TAG-01：不得以数字开头");
        assert_eq!(tags("#abc123").len(), 1, "数字可以出现在后面");
    }

    #[test]
    fn b14_hash_with_space_is_heading_not_tag() {
        assert!(tags("# 标题").is_empty(), "MD-TAG-04：`#`+空格 是标题");
        assert_eq!(tags("## 二级标题").len(), 0);
        assert_eq!(tags("正文中的 #真标签").len(), 1);
    }

    #[test]
    fn tag04_tags_inside_code_are_ignored() {
        assert!(tags("`#code` 行内代码").is_empty());
        assert!(tags("```\n#fenced\n```\n").is_empty());
    }

    #[test]
    fn tag02_terminates_at_punctuation() {
        let t = tags("#标签，后面 #另一个。");
        assert_eq!(
            t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            vec!["标签", "另一个"]
        );
    }

    #[test]
    fn tag05_norm_is_lowercase_but_name_keeps_case() {
        let t = tags("#Work");
        assert_eq!(t[0].name, "Work");
        assert_eq!(t[0].norm, "work");
    }

    #[test]
    fn tag06_frontmatter_tags_have_line_minus_one() {
        let t = from_frontmatter(&["a/b".to_string(), "#c".to_string()]);
        assert!(t.iter().all(|x| x.line == -1 && x.col == 0));
        assert!(t.iter().any(|x| x.name == "a/b"));
    }

    #[test]
    fn hash_inside_word_is_not_a_tag() {
        assert!(tags("abc#def").is_empty(), "起始边界不足");
    }
}
