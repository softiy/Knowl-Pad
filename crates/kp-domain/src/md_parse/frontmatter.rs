//! Frontmatter 解析（MD-FM-01 ~ MD-FM-06）。
//!
//! 规则要点：
//! - **MD-FM-01**：只有**第 1 行第 1 列**的 `---` 才是 frontmatter 起始；正文中间的 `---` 是水平分割线；
//! - **MD-FM-02**：缺闭合 `---` → 整块 frontmatter 无效，按普通正文处理并标记 `error`；
//! - **MD-FM-03**：YAML 解析失败**不得**跳过文件或中断，降级为「无 frontmatter」+ 警告，正文照常索引；
//! - **MD-FM-04**：保留 `tags`（字符串或字符串数组）、`aliases`（字符串数组）、`created`/`modified`（ISO 8601，仅展示）；
//! - **MD-FM-05**：未知字段原样保留（`raw`），写回时不得删除/重排/格式化；
//! - **MD-FM-06**：frontmatter 内容**不参与**全文索引（由调用方把该区间排除出 `plain_text`）。

use std::ops::Range;

/// frontmatter 的结构化视图（MD-FM-04）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    /// 原始 YAML 文本（MD-FM-05：原样保留，供写回）
    pub raw: String,
}

/// frontmatter 扫描结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontmatterScan {
    /// frontmatter 的字节区间（含首尾 `---` 行）；无 frontmatter 时为 None
    pub range: Option<Range<usize>>,
    /// 解析成功时的结构化数据；解析失败或无效时为 None
    pub data: Option<Frontmatter>,
    /// MD-FM-02：缺闭合 `---`
    pub unclosed: bool,
    /// MD-FM-03：YAML 解析失败（已降级）
    pub parse_failed: bool,
}

/// 扫描文本开头的 frontmatter。
pub fn scan(text: &str) -> FrontmatterScan {
    let mut out = FrontmatterScan::default();
    // MD-FM-01：必须第 1 行第 1 列就是 `---`（允许 BOM 已被上层剥离）
    if !text.starts_with("---") {
        return out;
    }
    // 首行必须是恰好 `---`（其后可有 ）
    let first_line_end = text.find('\n').map(|i| i + 1).unwrap_or(text.len());
    let first_line = text[..first_line_end].trim_end_matches(['\n', '\r', ' ', '\t']);
    if first_line != "---" {
        return out;
    }
    // 找闭合行：从第 2 行起，遇 `---`（或 `...`）即闭合
    let mut offset = first_line_end;
    let mut close: Option<(usize, usize)> = None; // (闭合行起始, 闭合行结束)
    while offset < text.len() {
        let line_end = text[offset..]
            .find('\n')
            .map(|i| offset + i + 1)
            .unwrap_or(text.len());
        let line = text[offset..line_end].trim_end_matches(['\n', '\r', ' ', '\t']);
        if line == "---" || line == "..." {
            close = Some((offset, line_end));
            break;
        }
        offset = line_end;
    }
    let Some((close_start, close_end)) = close else {
        // MD-FM-02：缺闭合 → 整块无效，按普通正文处理
        out.unclosed = true;
        return out;
    };
    let raw = &text[first_line_end..close_start];
    out.range = Some(0..close_end);
    match yaml_rust2::YamlLoader::load_from_str(raw) {
        Ok(docs) => {
            let mut fm = Frontmatter {
                raw: raw.to_string(),
                ..Default::default()
            };
            if let Some(doc) = docs.first() {
                fm.tags = string_list(doc, "tags");
                fm.aliases = string_list(doc, "aliases");
                fm.created = string_value(doc, "created");
                fm.modified = string_value(doc, "modified");
            }
            out.data = Some(fm);
        }
        Err(_) => {
            // MD-FM-03：降级为「无 frontmatter」，正文照常索引（区间仍排除出索引与链接扫描）
            out.parse_failed = true;
        }
    }
    out
}

/// 取字符串或字符串数组（MD-FM-04：`tags` 两种写法都要支持）。
fn string_list(doc: &yaml_rust2::Yaml, key: &str) -> Vec<String> {
    match doc[key].as_vec() {
        Some(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => doc[key]
            .as_str()
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
    }
}

/// 取单个字符串值。
fn string_value(doc: &yaml_rust2::Yaml, key: &str) -> Option<String> {
    doc[key].as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fm01_only_leading_rule_is_frontmatter() {
        assert!(
            scan("正文\n---\n不是 frontmatter\n").range.is_none(),
            "中间的 --- 是水平分割线"
        );
        assert!(scan("  ---\na: 1\n---\n").range.is_none(), "第 1 列才有效");
        assert!(scan("---\ntags: [a]\n---\n").range.is_some());
    }

    #[test]
    fn fm02_unclosed_is_invalid_and_treated_as_body() {
        let r = scan("---\ntags: [a]\n正文没有闭合\n");
        assert!(r.unclosed);
        assert!(r.range.is_none(), "缺闭合 → 整块按正文处理");
    }

    #[test]
    fn fm03_yaml_failure_degrades_without_failing() {
        let r = scan("---\n[未闭合的数组\n---\n正文\n");
        assert!(r.parse_failed);
        assert!(r.data.is_none());
        assert!(
            r.range.is_some(),
            "区间仍应排除出索引（避免把坏 YAML 当正文）"
        );
    }

    #[test]
    fn fm04_reads_tags_aliases_created_modified() {
        let r = scan("---\ntags: [a, b]\naliases:\n  - x\ncreated: 2026-01-02T03:04:05Z\n---\n");
        let fm = r.data.expect("应解析成功");
        assert_eq!(fm.tags, vec!["a", "b"]);
        assert_eq!(fm.aliases, vec!["x"]);
        assert_eq!(fm.created.as_deref(), Some("2026-01-02T03:04:05Z"));
        assert!(fm.modified.is_none());
    }

    #[test]
    fn fm04_single_string_tag_is_accepted() {
        let fm = scan("---\ntags: 单个\n---\n").data.expect("应解析");
        assert_eq!(fm.tags, vec!["单个"], "MD-FM-04：字符串写法也要支持");
    }

    #[test]
    fn fm05_unknown_fields_are_preserved_raw() {
        let fm = scan("---\ncustom: 1\ntags: [a]\n---\n")
            .data
            .expect("应解析");
        assert!(fm.raw.contains("custom: 1"), "MD-FM-05：未知字段原样保留");
        assert_eq!(fm.tags, vec!["a"]);
    }
}
