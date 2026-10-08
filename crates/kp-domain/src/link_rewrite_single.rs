//! **只改一处**的链接改写（FR-LINK-22 / AC-LINK-05：歧义链接逐条指定目标）。
//!
//! 与 link_rewrite::rewrite_references 的区别：那个按词干批量替换全部命中；
//! 这个只动**指定位置**的一处 —— 歧义链接必须逐个由用户指定目标，不能顺带改掉别的。
//! 从 link_rewrite.rs 拆出，使两个文件都低于 CODE-11 的 200 行警告线。

use crate::link_rewrite::{scan_links, LinkForm};

/// 与 `rewrite_references` 的区别：那个按**词干**批量替换全部命中，这个只动**指定位置**的一处 ——
/// 歧义链接必须逐个由用户指定目标，不能顺带改掉别的。
///
/// 位置口径与索引一致：**行号 1-based、列号 1-based**（`link` 表的 line/col）。返回（新全文, 改了几处）。
pub fn rewrite_occurrence_at(
    content: &str,
    line: u32,
    col: u32,
    new_target: &str,
) -> Option<(String, usize)> {
    let target = new_target.strip_suffix(".md").unwrap_or(new_target);
    let want_line = line.max(1) as usize;
    let want_col = col.max(1) as usize;
    for o in scan_links(content) {
        // 计算该链接起始处的行列（1-based）
        let before = &content[..o.start];
        let ln = before.chars().filter(|c| *c == '\n').count() + 1;
        let cl = before.chars().rev().take_while(|c| *c != '\n').count() + 1;
        if ln != want_line || cl != want_col {
            continue;
        }
        let replacement = match o.form {
            LinkForm::Wikilink | LinkForm::Embed => {
                let open = if o.form == LinkForm::Embed {
                    "![["
                } else {
                    "[["
                };
                let mut s = String::from(open);
                s.push_str(target);
                if let Some(a) = &o.anchor {
                    s.push_str(a);
                }
                if let Some(a) = &o.alias {
                    s.push('|');
                    s.push_str(a);
                }
                s.push_str("]]");
                s
            }
            LinkForm::MarkdownInline => {
                let whole = &content[o.start..o.end];
                let text_close = whole.find("](").unwrap_or(0);
                let mut s = String::from(&whole[..text_close]);
                s.push_str("](");
                s.push_str(target);
                s.push_str(".md");
                if let Some(x) = &o.trailing {
                    s.push_str(x);
                }
                s.push(')');
                s
            }
        };
        let mut out = String::with_capacity(content.len());
        out.push_str(&content[..o.start]);
        out.push_str(&replacement);
        out.push_str(&content[o.end..]);
        return Some((out, 1));
    }
    None
}
