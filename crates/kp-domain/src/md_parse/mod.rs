//! Markdown 结构化解析（技术方案 §5.1；PRD §3.1 的规则条款是唯一权威依据）。
//!
//! **纯函数约束（PRD:493）**：输入文件字节，输出 [`ParsedNote`]；无副作用、无 IO、无全局状态。
//! **永不抛错**：解析失败只记 `warnings`，绝不中断索引。
//!
//! 本切片（M3 PR-2 第一步）已实现：
//! - [`code_fence`]：围栏/缩进/行内代码区间（MD-WL-05 等规则的共同前提）；
//! - [`wikilink`]：MD-WL-01/02/03/04（仅提取，不裁决）/05/07/08/09。
//!
//! 后续切片：frontmatter（MD-FM-01~06）、标签（MD-TAG-01~06）、标题（MD-H-01~03）、
//! 块 ID（MD-BID-01~03）、plain_text 的完整清洗、10s 超时与 >5MB 跳过（由调用方负责）。

pub mod block_id;
pub mod code_fence;
pub mod frontmatter;
pub mod heading;
pub mod tag;
pub mod wikilink;

pub use block_id::BlockId;
pub use code_fence::CodeRanges;
pub use frontmatter::Frontmatter;
pub use heading::Heading;
pub use tag::Tag;
pub use wikilink::{Link, LinkKind};

/// 换行风格（NFR-PLAT-05：解析不得改写用户换行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

/// 解析结果（PRD §3.1.5 的 `ParsedNote`；本切片先落已实现字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNote {
    /// frontmatter（MD-FM-01~06；无效或缺闭合时为 None）
    pub frontmatter: Option<Frontmatter>,
    /// 标签（正文 + frontmatter；层级已展开，MD-TAG-03/06）
    pub tags: Vec<Tag>,
    /// 标题（ATX + Setext，MD-H-01~03）
    pub headings: Vec<Heading>,
    /// 块 ID 及其所属块行范围（MD-BID-01~03）
    pub block_ids: Vec<BlockId>,
    /// wikilink 与嵌入（原始形态，未裁决）
    pub links: Vec<Link>,
    /// 非致命警告（不中断索引）
    pub warnings: Vec<String>,
    /// 用于全文索引的纯文本（本切片：去掉代码区间后的正文）
    pub plain_text: String,
    /// 是否带 UTF-8 BOM（NFR-PLAT-06：保留，不改写）
    pub has_bom: bool,
    pub line_ending: LineEnding,
}

/// 解析一篇笔记。**永不失败**：非 UTF-8 输入按有损转换继续并记警告。
pub fn parse(bytes: &[u8]) -> ParsedNote {
    let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let body = if has_bom { &bytes[3..] } else { bytes };
    let mut warnings = Vec::new();
    let text = match std::str::from_utf8(body) {
        Ok(t) => t.to_string(),
        Err(_) => {
            warnings.push("INVALID_ENCODING：文件不是有效 UTF-8，已按有损转换继续解析".to_string());
            String::from_utf8_lossy(body).to_string()
        }
    };
    let line_ending = if text.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };
    // MD-FM-01~06：先扫 frontmatter；其区间既不进链接扫描（MD-WL-06）也不进全文索引（MD-FM-06）
    let fm = frontmatter::scan(&text);
    if fm.unclosed {
        warnings.push("FRONTMATTER_UNCLOSED：缺少闭合 ---，该块按正文处理".to_string());
    }
    if fm.parse_failed {
        warnings.push(
            "FRONTMATTER_INVALID：YAML 解析失败，已降级为无 frontmatter（正文照常索引）"
                .to_string(),
        );
    }
    let skip = fm.range.clone();
    let code = CodeRanges::scan(&text);
    let links = wikilink::extract_skipping(&text, &code, skip.clone());
    let headings = heading::extract(&text, &code, skip.clone());
    let block_ids = block_id::extract(&text, &code, skip.clone());
    let mut tags =
        tag::from_frontmatter(fm.data.as_ref().map(|f| f.tags.as_slice()).unwrap_or(&[]));
    tags.extend(tag::extract(&text, &code, skip.clone()));
    let plain_text = plain_text(&text, &code, skip);
    ParsedNote {
        frontmatter: fm.data,
        tags,
        headings,
        block_ids,
        links,
        warnings,
        plain_text,
        has_bom,
        line_ending,
    }
}

/// 去掉代码区间与 frontmatter 区间后的正文（MD-FM-06；完整清洗随后续切片完善）。
fn plain_text(text: &str, code: &CodeRanges, skip: Option<std::ops::Range<usize>>) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, ch) in text.char_indices() {
        let skipped = skip.as_ref().is_some_and(|r| r.contains(&i));
        if !skipped && !code.contains(i) {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_pure_and_never_fails_on_invalid_utf8() {
        let note = parse(&[0xff, 0xfe, b'a']);
        assert_eq!(note.warnings.len(), 1);
        assert!(note.warnings[0].contains("INVALID_ENCODING"));
    }

    #[test]
    fn bom_is_detected_and_links_still_parsed() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("[[A]]".as_bytes());
        let note = parse(&bytes);
        assert!(note.has_bom, "NFR-PLAT-06：BOM 需被识别并保留");
        assert_eq!(note.links.len(), 1);
        assert_eq!(note.links[0].target, "A");
    }

    #[test]
    fn crlf_is_detected() {
        let note = parse(b"a\r\nb\r\n");
        assert_eq!(note.line_ending, LineEnding::Crlf);
        assert_eq!(parse(b"a\nb\n").line_ending, LineEnding::Lf);
    }

    #[test]
    fn fm06_frontmatter_is_excluded_from_plain_text() {
        let note = parse("---\ntags: [a]\n秘密字段: 1\n---\n正文关键词\n".as_bytes());
        assert!(note.plain_text.contains("正文关键词"));
        assert!(
            !note.plain_text.contains("秘密字段"),
            "MD-FM-06：frontmatter 不进全文索引"
        );
        assert_eq!(
            note.frontmatter.as_ref().map(|f| f.tags.clone()),
            Some(vec!["a".to_string()])
        );
    }

    #[test]
    fn wl06_wikilinks_inside_frontmatter_are_not_parsed() {
        let note = parse("---\naliases: [\"[[NotALink]]\"]\n---\n正文 [[Real]]\n".as_bytes());
        assert_eq!(
            note.links.len(),
            1,
            "MD-WL-06：frontmatter 内的 [[...]] 不解析"
        );
        assert_eq!(note.links[0].target, "Real");
    }

    #[test]
    fn fm02_unclosed_frontmatter_keeps_body_parseable() {
        let note = parse("---\ntags: [a]\n[[InBody]]\n".as_bytes());
        assert!(note.frontmatter.is_none());
        assert!(note
            .warnings
            .iter()
            .any(|w| w.contains("FRONTMATTER_UNCLOSED")));
        assert_eq!(note.links.len(), 1, "缺闭合时该块按正文处理，链接照常解析");
    }

    #[test]
    fn plain_text_drops_code_ranges() {
        let note = parse(b"keep\n```\ndrop\n```\nkeep2\n");
        assert!(note.plain_text.contains("keep"));
        assert!(!note.plain_text.contains("drop"));
    }
    #[test]
    fn tag06_frontmatter_tags_flow_through_parse() {
        let note = parse("---\ntags: [甲/乙]\n---\n正文 #丙\n".as_bytes());
        let names: Vec<&str> = note.tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"甲/乙"), "frontmatter 标签要登记");
        assert!(names.contains(&"甲"), "层级展开要包含祖先");
        assert!(names.contains(&"丙"), "正文标签也要登记");
        let fm_tag = note.tags.iter().find(|t| t.name == "甲/乙").expect("存在");
        assert_eq!(fm_tag.line, -1, "MD-TAG-06：frontmatter 来源 line = -1");
    }

    #[test]
    fn code_blocks_hide_tags_headings_and_block_ids() {
        let note = parse("```\n# tag\n```\n# 真标题\n文字 ^bid\n".as_bytes());
        assert_eq!(note.headings.len(), 1);
        assert_eq!(note.headings[0].text, "真标题");
        assert!(note.tags.is_empty(), "代码块内的 #tag 不算标签");
        assert_eq!(note.block_ids.len(), 1);
    }
}
