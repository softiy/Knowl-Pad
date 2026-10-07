//! Wikilink 提取（MD-WL-01 ~ MD-WL-09）。
//!
//! **不判定有效性**（MD-WL-03）：resolved/dangling/ambiguous 由索引阶段的链接裁决产生，
//! 本层只负责「把链接从正文里准确地抠出来」，并给出目标/锚点/别名/类型/行列号。
//! 未覆盖：MD-WL-06（frontmatter 内的 wikilink 不解析）——frontmatter 解析属后续切片。

use super::code_fence::CodeRanges;

/// 链接类型（MD-WL-07）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[[目标]]`
    Link,
    /// `![[目标]]`（嵌入，与普通链接分别统计）
    Embed,
}

/// 一个 wikilink 的**原始形态**（未裁决）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// 目标引用（已去首尾空白，MD-WL-09）；不含 `.md` 扩展名（MD-WL-02）
    pub target: String,
    /// `#` 后的锚点（标题或 `^块ID`）
    pub anchor: Option<String>,
    /// `|` 后的显示别名
    pub alias: Option<String>,
    pub kind: LinkKind,
    /// 1-based 行号
    pub line: u32,
    /// 1-based 列号（按**字符**计，非字节）
    pub col: u32,
}

/// 提取全部 wikilink（按出现顺序）。
pub fn extract(text: &str, code: &CodeRanges) -> Vec<Link> {
    extract_skipping(text, code, None)
}

/// 同上，但跳过指定字节区间内的内容（用于 **MD-WL-06**：frontmatter 内的 wikilink 不解析）。
pub fn extract_skipping(
    text: &str,
    code: &CodeRanges,
    skip: Option<std::ops::Range<usize>>,
) -> Vec<Link> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] != b'[' || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        // MD-WL-08：被转义的 \[[ 不解析（反斜杠个数为奇数即转义）
        let mut backslashes = 0usize;
        let mut k = i;
        while k > 0 && bytes[k - 1] == b'\\' {
            backslashes += 1;
            k -= 1;
        }
        if backslashes % 2 == 1 {
            i += 2;
            continue;
        }
        // MD-WL-07：紧邻的 `!` 表示嵌入
        let embed = i > 0 && bytes[i - 1] == b'!';
        let start = if embed { i - 1 } else { i };
        // MD-WL-05：代码块/行内代码内不解析
        let in_skip = skip.as_ref().is_some_and(|r| r.contains(&start));
        if code.contains(start) || in_skip {
            i += 2;
            continue;
        }
        let Some(close) = find_close(bytes, i + 2) else {
            i += 2;
            continue;
        };
        let inner = &text[i + 2..close];
        // 嵌套 `[[` 视为非法，跳过
        if inner.contains('[') || code.contains(close) {
            i += 2;
            continue;
        }
        if let Some(link) = build(inner, embed, text, start) {
            out.push(link);
            i = close + 2;
            continue;
        }
        i += 2;
    }
    out
}

/// 找 `]]` 的字节位置（不在同层 `[` 之后）。
fn find_close(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 拆分内层文本：`目标#锚点|别名`（MD-WL-01），目标为空则整条丢弃。
fn build(inner: &str, embed: bool, text: &str, start: usize) -> Option<Link> {
    let (head, alias) = match inner.split_once('|') {
        Some((h, a)) => (h, Some(a.trim().to_string())),
        None => (inner, None),
    };
    let (target, anchor) = match head.split_once('#') {
        Some((t, a)) => (t.trim(), Some(a.trim().to_string())),
        None => (head.trim(), None),
    };
    if target.is_empty() {
        return None;
    }
    let (line, col) = line_col(text, start);
    Some(Link {
        target: target.to_string(),
        anchor: anchor.filter(|a| !a.is_empty()),
        alias: alias.filter(|a| !a.is_empty()),
        kind: if embed {
            LinkKind::Embed
        } else {
            LinkKind::Link
        },
        line,
        col,
    })
}

/// 由字节位置算 1-based 行列（列按字符计）。
fn line_col(text: &str, byte: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut line_start = 0usize;
    for (i, ch) in text.char_indices() {
        if i >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = text[line_start..byte].chars().count() as u32 + 1;
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<Link> {
        extract(text, &CodeRanges::scan(text))
    }

    #[test]
    fn b01_simple_wikilink() {
        let l = links("见 [[Note]] 处");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].target, "Note");
        assert_eq!(l[0].kind, LinkKind::Link);
        assert_eq!((l[0].line, l[0].col), (1, 3));
    }

    #[test]
    fn b02_alias_and_b03_anchor() {
        let l = links("[[Note|别名]] 与 [[Note#标题]] 与 [[Note#^abc123]]");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].alias.as_deref(), Some("别名"));
        assert_eq!(l[1].anchor.as_deref(), Some("标题"));
        assert_eq!(l[2].anchor.as_deref(), Some("^abc123"));
    }

    #[test]
    fn b05_path_target_and_wl09_trim() {
        let l = links("[[  folder/sub/Note  ]]");
        assert_eq!(l[0].target, "folder/sub/Note", "MD-WL-09 去首尾空白");
    }

    #[test]
    fn b06_embed_kind() {
        let l = links("![[image.png]] 与 ![[Note#标题]]");
        assert_eq!(l.len(), 2);
        assert!(l.iter().all(|x| x.kind == LinkKind::Embed));
    }

    #[test]
    fn b08_b09_b10_not_parsed_in_code_or_escaped() {
        assert!(
            links("行内 `[[Note]]` 代码").is_empty(),
            "MD-WL-05 行内代码"
        );
        assert!(links("```\n[[Note]]\n```\n").is_empty(), "MD-WL-05 代码块");
        assert!(links("\\[[Note]]").is_empty(), "MD-WL-08 转义");
    }

    #[test]
    fn empty_target_is_dropped() {
        assert!(links("[[|别名]] [[#锚点]]").is_empty());
    }

    #[test]
    fn multiline_line_numbers_are_correct() {
        let l = links("第一行\n第二行 [[A]]\n[[B]]");
        assert_eq!((l[0].line, l[1].line), (2, 3));
    }
}
