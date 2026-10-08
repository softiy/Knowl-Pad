//! 链接改写：扫描（哪些链接该改）+ 生成（改成什么）—— 纯函数，不做 IO。
//!
//! 技术方案 §1.3.3 把这条路径标为最高风险路径、§6.1 标为最高风险模块：
//! 写错就是用户的文件被损坏。因此本模块只做纯计算；备份、原子写与回滚在命令层。
//!
//! 受保护区域一律不动（AC-FILE-01 的硬要求）：
//! 一 frontmatter（文件开头的三横线块）二 围栏代码块 三 行内代码 四 HTML 注释。

use crate::link_rewrite_protect::{in_protected, protected_ranges};

/// 链接形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkForm {
    Wikilink,
    Embed,
    MarkdownInline,
}

/// 一处链接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOccurrence {
    pub start: usize,
    pub end: usize,
    pub form: LinkForm,
    pub target: String,
    pub alias: Option<String>,
    pub anchor: Option<String>,
    pub trailing: Option<String>,
}

/// 扫描出所有可改写的链接（受保护区域内的不计入）。
pub fn scan_links(content: &str) -> Vec<LinkOccurrence> {
    let prot = protected_ranges(content);
    let mut out: Vec<LinkOccurrence> = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let embed = bytes[i..].starts_with(b"![[");
        let plain = !embed && bytes[i..].starts_with(b"[[");
        if embed || plain {
            let open_len = if embed { 3 } else { 2 };
            if let Some(close_rel) = content[i + open_len..].find("]]") {
                let end = i + open_len + close_rel + 2;
                let inner = &content[i + open_len..i + open_len + close_rel];
                if !in_protected(&prot, i, end) {
                    let (target_part, alias) = match inner.split_once('|') {
                        Some(pair) => (pair.0, Some(pair.1.to_string())),
                        None => (inner, None),
                    };
                    let (target, anchor) = match target_part.split_once('#') {
                        Some(pair) => (pair.0.to_string(), Some(format!("#{}", pair.1))),
                        None => (target_part.to_string(), None),
                    };
                    out.push(LinkOccurrence {
                        start: i,
                        end,
                        form: if embed {
                            LinkForm::Embed
                        } else {
                            LinkForm::Wikilink
                        },
                        target,
                        alias,
                        anchor,
                        trailing: None,
                    });
                }
                i = end;
                continue;
            }
        }
        if bytes[i] == b'[' && !bytes[i..].starts_with(b"[[") {
            if let Some(close) = content[i..].find("](") {
                let text_end = i + close;
                if let Some(paren_rel) = content[text_end + 2..].find(')') {
                    let end = text_end + 2 + paren_rel + 1;
                    let inside = &content[text_end + 2..text_end + 2 + paren_rel];
                    if !in_protected(&prot, i, end) {
                        let (target, trailing) = match inside.find(|c: char| c.is_whitespace()) {
                            Some(p) => (inside[..p].to_string(), Some(inside[p..].to_string())),
                            None => (inside.to_string(), None),
                        };
                        out.push(LinkOccurrence {
                            start: i,
                            end,
                            form: LinkForm::MarkdownInline,
                            target,
                            alias: None,
                            anchor: None,
                            trailing,
                        });
                    }
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// 把新引用套上原目标所在的目录前缀（FR-LINK-23 的「完整相对路径」形式保持不变）。
fn with_dir_of(original: &str, new_stem: &str) -> String {
    match original.rsplit_once('/') {
        Some(parts) => format!("{}/{}", parts.0, new_stem),
        None => new_stem.to_string(),
    }
}

/// 目标引用的词干：去目录、去 .md、小写（FR-LINK-02 二 的大小写不敏感）。
pub fn stem_of(reference: &str) -> String {
    let last = reference.rsplit('/').next().unwrap_or(reference);
    last.strip_suffix(".md").unwrap_or(last).to_lowercase()
}

/// 把 from 改写成 to：按词干大小写不敏感匹配，保留别名、锚点、标题与形式。
///
/// 返回新全文与实际改写处数；一处没改时返回原文与 0。
pub fn rewrite_references(content: &str, from: &str, to: &str) -> (String, usize) {
    let from_stem = stem_of(from);
    let to_stem = to.rsplit('/').next().unwrap_or(to);
    let to_stem = to_stem.strip_suffix(".md").unwrap_or(to_stem).to_string();
    let occ = scan_links(content);
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    let mut changed = 0usize;
    for o in occ {
        if stem_of(&o.target) != from_stem {
            continue;
        }
        out.push_str(&content[cursor..o.start]);
        let replacement = match o.form {
            LinkForm::Wikilink | LinkForm::Embed => {
                let open = if o.form == LinkForm::Embed {
                    "![["
                } else {
                    "[["
                };
                let mut s = String::from(open);
                s.push_str(&with_dir_of(&o.target, &to_stem));
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
                s.push_str(&with_dir_of(&o.target, &to_stem));
                s.push_str(".md");
                if let Some(t) = &o.trailing {
                    s.push_str(t);
                }
                s.push(')');
                s
            }
        };
        out.push_str(&replacement);
        cursor = o.end;
        changed += 1;
    }
    out.push_str(&content[cursor..]);
    (out, changed)
}
