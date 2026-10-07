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

pub mod code_fence;
pub mod wikilink;

pub use code_fence::CodeRanges;
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
    let code = CodeRanges::scan(&text);
    let links = wikilink::extract(&text, &code);
    let plain_text = plain_text(&text, &code);
    ParsedNote {
        links,
        warnings,
        plain_text,
        has_bom,
        line_ending,
    }
}

/// 去掉代码区间后的正文（本切片的最小实现；完整清洗随后续切片完善）。
fn plain_text(text: &str, code: &CodeRanges) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, ch) in text.char_indices() {
        if !code.contains(i) {
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
    fn plain_text_drops_code_ranges() {
        let note = parse(b"keep\n```\ndrop\n```\nkeep2\n");
        assert!(note.plain_text.contains("keep"));
        assert!(!note.plain_text.contains("drop"));
    }
}
