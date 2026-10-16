//! 标签改写的**纯逻辑**（FR-TAG-07/08；AC-TAG-02）。
//!
//! 拆分口径（与链接改写一致）：**planner 独立**（本模块），而"备份 → 原子写 → 失败整批回滚"的骨架
//! 复用 `kp_domain::link_rewrite_apply`（它的 `RewritePlan` / `FileChange` 本来就是通用形状）。
//!
//! 三条需求原文决定这里的形状：
//! - FR-TAG-07「将 Vault 内某标签的全部出现改写为新名称（**含层级子标签的前缀更新**）」
//!   ⇒ 改写 `a` 时 `#a/b` 要变成 `#x/b`（前缀替换，不只是命中的那一个）；
//! - AC-TAG-02「80 篇笔记中全部 `#old/name` 被改写为 `#new/name`；**frontmatter 中的 `tags:` 值同步更新**」
//!   ⇒ 正文与 frontmatter **两条来源都要覆盖**；
//! - MD-TAG-06「frontmatter 的标签 `line = -1`（**无行列号**）」
//!   ⇒ 正文按解析器给的行列号改写，frontmatter 只能**按块扫**（本模块最容易写错的地方）。

use crate::error::AppError;
use crate::md_parse;

/// 一个文件里的改写结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRewriteOutcome {
    /// 改写后的全文（没有任何命中时与输入**逐字节相同**）。
    pub new_content: String,
    /// 实际改写的**处数**（正文 + frontmatter 的标签出现次数合计）。
    pub hits: usize,
}

/// FR-TAG-08：新名称不得形成**非法嵌套或循环**。返回用户可读的中文原因（ERR-02）。
///
/// 判据（每条都有用例）：空名称 / 首尾斜杠 / 连续斜杠 / 含空白 ⇒ 非法嵌套；
/// 新名称落在**自身路径之下**（`a` → `a/b`）⇒ 循环（自己成了自己的祖先）。
pub fn validate_rename(from_norm: &str, to: &str) -> Result<(), AppError> {
    let name = to.trim();
    if name.is_empty() {
        return Err(AppError::InvalidFilename("标签名不能为空".to_string()));
    }
    if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
        return Err(AppError::InvalidFilename(format!(
            "标签名不能以斜杠开头或结尾、也不能出现连续斜杠：{name}"
        )));
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err(AppError::InvalidFilename(format!(
            "标签名不能包含空白：{name}"
        )));
    }
    let to_norm = name.to_lowercase();
    if !from_norm.is_empty() && to_norm.starts_with(&format!("{from_norm}/")) {
        return Err(AppError::InvalidFilename(format!(
            "新名称 {name} 位于旧标签 {from_norm} 之下，会形成循环"
        )));
    }
    Ok(())
}

/// 该标签（norm）是否命中本次改名；命中则给出**替换后的名称**（含前缀更新，FR-TAG-07）。
fn replacement_for(norm: &str, from_norm: &str, to: &str) -> Option<String> {
    if norm == from_norm {
        return Some(to.to_string());
    }
    norm.strip_prefix(&format!("{from_norm}/"))
        .map(|rest| format!("{to}/{rest}"))
}

/// 把 content 里的 `#from_norm`（**含层级子孙**）改写成 `to`（正文与 frontmatter 都改）。
///
/// 大小写：匹配用 norm（小写规范键，MD-TAG-05），**写入用调用方给的 to 原文**。
pub fn rewrite_tags(content: &str, from_norm: &str, to: &str) -> TagRewriteOutcome {
    let unchanged = TagRewriteOutcome {
        new_content: content.to_string(),
        hits: 0,
    };
    if from_norm.is_empty() {
        return unchanged;
    }
    let mut lines: Vec<String> = content.split('\n').map(|l| l.to_string()).collect();

    // ① frontmatter 先做：它只改块内的行，不影响正文的行列号。
    let mut fm_hits = 0;
    if let Some(range) = md_parse::frontmatter::scan(content).range {
        let (first, last) = byte_range_to_lines(content, range.start, range.end);
        fm_hits = rewrite_frontmatter_lines(&mut lines, first, last, from_norm, to);
    }

    // ② 正文：用解析器的行列号。
    let parsed = md_parse::parse(content.as_bytes());
    let mut edits: Vec<(usize, usize, usize, String)> = Vec::new();
    for tag in &parsed.tags {
        // MD-TAG-06：frontmatter 的条目没有行列号（line = -1）⇒ 上面已处理，这里跳过。
        if tag.line < 1 {
            continue;
        }
        if let Some(new_name) = replacement_for(&tag.norm, from_norm, to) {
            edits.push((
                tag.line as usize,
                tag.col as usize,
                tag.name.chars().count(),
                new_name,
            ));
        }
    }
    // **按源位置去重**：解析器对层级标签会**同时给出祖先条目与自身条目**（MD-TAG-03），
    // 两者是**同一个源位置**（同一 line/col），但祖先条目的名字更短（old vs old/name）。
    // 不去重就会对同一段文本替换两次：轻则重复计数，重则第二次用旧列偏移覆盖已改写的内容 ⇒ 损坏文本。
    // 口径：**一个源位置 = 一处**，保留**最长**的那个（= 真正写下的那个标签）。
    edits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(b.2.cmp(&a.2)));
    edits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let body_hits = edits.len();
    // 同一行内可能有多处 ⇒ **从右往左**替换，否则左侧替换会让右侧的列偏移失效（经典 off-by-N）。
    edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (line_no, col, old_len, new_name) in edits {
        if line_no == 0 || line_no > lines.len() {
            continue;
        }
        let chars: Vec<char> = lines[line_no - 1].chars().collect();
        if col + old_len > chars.len() {
            continue;
        }
        let mut next: String = chars[..col].iter().collect();
        next.push_str(&new_name);
        next.extend(chars[col + old_len..].iter());
        lines[line_no - 1] = next;
    }

    let new_content = lines.join("\n");
    if new_content == content {
        // 新旧同名等"命中但没变"的情形按 0 处理，避免把空操作算成改写。
        return unchanged;
    }
    TagRewriteOutcome {
        new_content,
        hits: fm_hits + body_hits,
    }
}

/// 把字节偏移换算成 0 基的行号区间（frontmatter 块的字节范围 → 行范围）。
fn byte_range_to_lines(content: &str, start: usize, end: usize) -> (usize, usize) {
    let line_of = |offset: usize| content[..offset.min(content.len())].matches('\n').count();
    (line_of(start), line_of(end))
}

/// 在 frontmatter 块的行区间里改写 tags 键的值，返回改动**处数**。
///
/// 三种写法都认（MD-FM-04 要求字符串与字符串数组都支持）：流式数组 `[a, b/c]`、单值 `a`、
/// 块状数组（`tags:` 后跟若干 `- a` 行）。**块外的行一律不碰**。
fn rewrite_frontmatter_lines(
    lines: &mut [String],
    first: usize,
    last: usize,
    from_norm: &str,
    to: &str,
) -> usize {
    let mut hits = 0;
    let mut in_block_list = false;
    let mut index = first;
    while index < lines.len() && index <= last {
        let line = lines[index].clone();
        let trimmed = line.trim_start().to_string();
        if in_block_list {
            if let Some(rest) = trimmed.strip_prefix('-') {
                if let Some(new_item) = rewrite_token(rest, from_norm, to) {
                    hits += 1;
                    lines[index] = replace_tail(&line, &format!("-{new_item}"));
                }
                index += 1;
                continue;
            }
            in_block_list = false;
        }
        if let Some(rest) = strip_key(&trimmed, "tags") {
            // 保留冒号后的原空白（`tags: [a]` 不能变成 `tags:[a]`）。
            let head_ws_len = rest.len() - rest.trim_start().len();
            let head_ws = rest[..head_ws_len].to_string();
            let value = rest.trim().to_string();
            if value.is_empty() {
                // 块状数组：后续的 `- x` 行交给上面的分支。
                in_block_list = true;
            } else {
                let (new_value, n) = rewrite_flow_list(&value, from_norm, to);
                hits += n;
                if n > 0 {
                    lines[index] = replace_tail(&line, &format!("tags:{head_ws}{new_value}"));
                }
            }
        }
        index += 1;
    }
    hits
}

/// 行首是 `<key>:` 时返回冒号后的部分（含原空白）。
fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix(key)?.strip_prefix(':')
}

/// 保留行首缩进，替换掉 `key:` 之后的内容。
fn replace_tail(line: &str, tail: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    format!("{}{}", &line[..indent_len], tail)
}

/// 改写 `[a, b/c]` 或 `a` 这类值；返回（新值, 改动处数）。
fn rewrite_flow_list(value: &str, from_norm: &str, to: &str) -> (String, usize) {
    let bracketed = value.starts_with('[') && value.ends_with(']') && value.len() >= 2;
    let inner = if bracketed {
        &value[1..value.len() - 1]
    } else {
        value
    };
    let mut hits = 0;
    let parts: Vec<String> = inner
        .split(',')
        .map(|part| match rewrite_token(part, from_norm, to) {
            Some(new_token) => {
                hits += 1;
                new_token
            }
            None => part.to_string(),
        })
        .collect();
    let joined = parts.join(",");
    if bracketed {
        (format!("[{joined}]"), hits)
    } else {
        (joined, hits)
    }
}

/// 改写一个"取值片段"（可能是 ` a`、`a `、带引号的 `\"a\"`）；不匹配则返回 None。
fn rewrite_token(text: &str, from_norm: &str, to: &str) -> Option<String> {
    let head_len = text.len() - text.trim_start().len();
    let tail_len = text.len() - text.trim_end().len();
    let core = text.trim();
    if core.is_empty() {
        return None;
    }
    let quoted = core.len() >= 2
        && ((core.starts_with('"') && core.ends_with('"'))
            || (core.starts_with('\'') && core.ends_with('\'')));
    let bare = if quoted {
        &core[1..core.len() - 1]
    } else {
        core
    };
    let name = bare.trim().trim_start_matches('#');
    let new_name = replacement_for(&name.to_lowercase(), from_norm, to)?;
    let written = if quoted {
        format!("{}{}{}", &core[..1], new_name, &core[core.len() - 1..])
    } else {
        new_name
    };
    Some(format!(
        "{}{}{}",
        &text[..head_len],
        written,
        &text[text.len() - tail_len..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正文标签改写（含同一行多处）。
    #[test]
    fn rewrites_body_tags() {
        let out = rewrite_tags("甲 #old/name 乙 #old/name 丙\n", "old/name", "new/name");
        assert_eq!(out.new_content, "甲 #new/name 乙 #new/name 丙\n");
        assert_eq!(out.hits, 2);
    }

    /// **FR-TAG-07 的前缀更新**：改 `old` 时 `#old/name` 要变成 `#new/name`。
    #[test]
    fn prefix_update_covers_descendants() {
        let out = rewrite_tags("#old 与 #old/name 与 #oldname\n", "old", "new");
        assert_eq!(
            out.new_content, "#new 与 #new/name 与 #oldname\n",
            "孙标签按前缀更新；#oldname 不是 old 的子孙（无斜杠）⇒ 不动"
        );
        assert_eq!(out.hits, 2);
    }

    /// 大小写：匹配走 norm（小写），写入用调用方给的原文。
    #[test]
    fn matching_is_case_insensitive_and_writes_the_given_name() {
        let out = rewrite_tags("#Work 甲\n#WORK 乙\n", "work", "NewName");
        assert_eq!(out.new_content, "#NewName 甲\n#NewName 乙\n");
        assert_eq!(out.hits, 2);
    }

    /// **AC-TAG-02：frontmatter 的 tags 同步更新**（断言逐字节相等）。
    ///
    /// 为什么必须逐字节：早先这里只断言 `contains("new/name")`，正文那处命中就能让它通过，
    /// 从而**盖住 frontmatter 根本没被改写**这个真问题（见下一条用例）。
    #[test]
    fn rewrites_frontmatter_tags_too() {
        let src = "---\ntags: [old/name, 其他]\n---\n\n正文 #old/name 出现\n";
        let out = rewrite_tags(src, "old/name", "new/name");
        assert_eq!(
            out.new_content,
            "---\ntags: [new/name, 其他]\n---\n\n正文 #new/name 出现\n"
        );
        assert_eq!(out.hits, 2, "frontmatter 一处 + 正文一处");
    }

    /// **只有 frontmatter 有标签的文件**（AC-TAG-02 的"c.md 场景"）。
    ///
    /// 这条用例暴露过一个**真 bug**：MD-TAG-06 规定 frontmatter 标签没有行列号（line = -1），
    /// 而最初实现只按行列号改写 ⇒ 这类文件**一处也没改**，还被算成"没有命中"而不进改写计划。
    #[test]
    fn rewrites_frontmatter_only_file() {
        let src = "---\ntags: [old/name, 其他]\n---\n正文无标签\n";
        let out = rewrite_tags(src, "old/name", "new/name");
        assert_eq!(
            out.new_content,
            "---\ntags: [new/name, 其他]\n---\n正文无标签\n"
        );
        assert_eq!(out.hits, 1);
    }

    /// frontmatter 的三种写法：单值、块状数组、带引号（MD-FM-04：字符串与字符串数组都支持）。
    #[test]
    fn rewrites_all_frontmatter_shapes() {
        let single = rewrite_tags("---\ntags: old\n---\n", "old", "new");
        assert_eq!(single.new_content, "---\ntags: new\n---\n");
        assert_eq!(single.hits, 1);

        let block = rewrite_tags("---\ntags:\n  - old\n  - 其他\n---\n", "old", "new");
        assert_eq!(block.new_content, "---\ntags:\n  - new\n  - 其他\n---\n");
        assert_eq!(block.hits, 1);

        let quoted = rewrite_tags("---\ntags: [\"old\"]\n---\n", "old", "new");
        assert_eq!(quoted.new_content, "---\ntags: [\"new\"]\n---\n");
        assert_eq!(quoted.hits, 1);
    }

    /// frontmatter 的前缀更新（FR-TAG-07 同样适用于 tags 数组里的层级标签）。
    #[test]
    fn frontmatter_prefix_update() {
        let out = rewrite_tags("---\ntags: [old, old/子]\n---\n", "old", "new");
        assert_eq!(out.new_content, "---\ntags: [new, new/子]\n---\n");
        assert_eq!(out.hits, 2);
    }

    /// **只改块内的行**：正文里 `tags:` 字样的行不受 frontmatter 改写逻辑影响。
    #[test]
    fn body_lines_that_look_like_frontmatter_are_untouched() {
        let src = "---\ntags: [old]\n---\n\ntags: [old] 这行是正文\n";
        let out = rewrite_tags(src, "old", "new");
        assert_eq!(
            out.new_content, "---\ntags: [new]\n---\n\ntags: [old] 这行是正文\n",
            "只有块内的那行该被改"
        );
    }

    /// 没有命中 ⇒ 与输入**逐字节相同**（planner 据此把该文件排除在计划外）。
    #[test]
    fn no_hit_leaves_content_byte_identical() {
        let src = "无标签正文\n#别的 标签\n";
        let out = rewrite_tags(src, "old", "new");
        assert_eq!(out.new_content, src);
        assert_eq!(out.hits, 0);
    }

    /// 新旧同名 ⇒ 0 命中（不把空操作算成改写）。
    #[test]
    fn renaming_to_the_same_name_is_a_no_op() {
        let out = rewrite_tags("#same 甲\n", "same", "same");
        assert_eq!(out.hits, 0);
    }

    /// FR-TAG-08 的校验：非法嵌套与循环。
    #[test]
    fn validate_rejects_illegal_nesting_and_cycles() {
        assert!(validate_rename("old", "new/name").is_ok());
        assert!(validate_rename("old", "").is_err(), "空名称");
        assert!(validate_rename("old", "  ").is_err(), "纯空白");
        assert!(validate_rename("old", "/a").is_err(), "以斜杠开头");
        assert!(validate_rename("old", "a/").is_err(), "以斜杠结尾");
        assert!(validate_rename("old", "a//b").is_err(), "连续斜杠");
        assert!(validate_rename("old", "a b").is_err(), "含空白");
        assert!(
            validate_rename("old", "old/child").is_err(),
            "循环：落到自身之下"
        );
        assert!(
            validate_rename("old", "OLD/child").is_err(),
            "循环判定要大小写不敏感（norm 口径）"
        );
    }
}
