//! Wikilink 提取（MD-WL-01 ~ MD-WL-09）。
//!
//! **不判定有效性**（MD-WL-03）：resolved/dangling/ambiguous 由索引阶段的链接裁决产生，
//! 本层只负责「把链接从正文里准确地抠出来」，并给出目标/锚点/别名/类型/行列号。
//! 未覆盖：MD-WL-06（frontmatter 内的 wikilink 不解析）——frontmatter 解析属后续切片。

use super::code_fence::CodeRanges;
use super::ParseAborted;

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

/// 中止检查的迭代步长（DEBT-32 的 P0 回归修法：让预算在**热循环内部**生效）。
///
/// 每 64 次主循环检查一次（一次闭包调用 ≈ 纳秒级）：既让病态输入在毫秒级停下，又不拖慢正常解析。
/// 取 64 而**不是** 4096 的理由：单次 `find_close` 可能是 O(n)（后面没有再出现的 `]]` 时它要扫到结尾），
/// 步长太大会让"超预算"的量级退回成"若干次 O(n)"。
const ABORT_CHECK_EVERY: usize = 64;

/// 提取全部 wikilink（按出现顺序）。**无预算版本**：不做中止检查（行为与改动前逐字节相同）。
pub fn extract(text: &str, code: &CodeRanges) -> Vec<Link> {
    extract_skipping(text, code, None)
}

/// 同上，但跳过指定字节区间内的内容（用于 **MD-WL-06**：frontmatter 内的 wikilink 不解析）。
///
/// 无预算版本 —— 生产解析路径用 [`extract_skipping_guarded`]。
pub fn extract_skipping(
    text: &str,
    code: &CodeRanges,
    skip: Option<std::ops::Range<usize>>,
) -> Vec<Link> {
    // never_abort 恒 false ⇒ 只可能是 Ok；Err 回落成空结果而不是 panic（理由同 `parse` 的注释）。
    extract_skipping_guarded(text, code, skip, &never_abort).unwrap_or_default()
}

fn never_abort() -> bool {
    false
}

/// 提取全部 wikilink，并在 `abort()` 为真时**立刻中止**（SEC-03 / DEBT-32）。
///
/// **P0 回归的教训（2026-10-17）**：上一版只在"阶段之间"检查预算，而 `line_col` 的 O(n²) 爆炸发生在
/// **某个阶段内部** —— 于是一份 0.75 MB / 5 万条 wikilink 的笔记会先烧掉数分钟 CPU 才轮到一个检查点，
/// 用户看到的是"索引一直不结束"。现在中止检查放在主循环里（每 [`ABORT_CHECK_EVERY`] 次迭代），
/// **任何分支**（含未闭合的 `[[`）都能在毫秒级停下。
///
/// **行列号是扫描时增量维护的（DEBT-42）**：旧实现给每个链接都调一次 `line_col`（从文件头扫到该链接），
/// 实测 100 KB / 4000 条 = 2.8 s、按平方外推到 0.75 MB / 5 万条 ≈ 7 分钟。现在 `(line, col)` 只随
/// **单调前进的 `cursor`** 累加（字符数按 UTF-8 首字节计，换行清零）⇒ O(1)/链接，且结果与旧实现逐字段相同
/// （用例 `wikilink_positions_match_pre_refactor_baseline` 用修复前采样的基准断言）。
pub fn extract_skipping_guarded(
    text: &str,
    code: &CodeRanges,
    skip: Option<std::ops::Range<usize>>,
    abort: &dyn Fn() -> bool,
) -> Result<Vec<Link>, ParseAborted> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    // 行列游标：只前进、不回退（链接的 start 随 i 单调递增；`advance` 里有 debug_assert 兜底）
    let mut cursor = 0usize;
    let mut line = 1u32;
    let mut col_in_line = 0u32;
    let mut tick = 0usize;
    while i + 1 < bytes.len() {
        tick += 1;
        if tick >= ABORT_CHECK_EVERY {
            tick = 0;
            if abort() {
                return Err(ParseAborted);
            }
        }
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
            // **没有更多链接可言**：`find_close` 返回 None 表示 `[i+2, len)` 内不存在 `]]`，
            // 而后续每个 `[[` 的搜索区间都是它的后缀 ⇒ 必然同样失败。旧实现会继续扫（未闭合 `[[`
            // 因此是 O(n²)）；现在直接收工 —— 语义相同（谁也变不出链接），代价 O(1)。
            return Ok(out);
        };
        let inner = &text[i + 2..close];
        // 嵌套 `[[` 视为非法，跳过
        if inner.contains('[') || code.contains(close) {
            i += 2;
            continue;
        }
        advance(bytes, start, &mut cursor, &mut line, &mut col_in_line);
        if let Some(link) = build(inner, embed, line, col_in_line + 1) {
            out.push(link);
            i = close + 2;
            continue;
        }
        i += 2;
    }
    Ok(out)
}

/// 把行列游标推进到 `target`（**调用方保证 target 单调不减**；`start` 随 `i` 递增）。
///
/// 列按**字符**计（`Link.col` 的契约）：只数 UTF-8 首字节（`b & 0b1100_0000 != 0b1000_0000`），
/// 这样每个字节最多看一次 —— 合计 O(文本长度)，而不是旧实现的 O(链接数 × 文件长度)。
fn advance(bytes: &[u8], target: usize, cursor: &mut usize, line: &mut u32, col_in_line: &mut u32) {
    debug_assert!(target >= *cursor, "行列游标只能前进");
    while *cursor < target {
        let b = bytes[*cursor];
        *cursor += 1;
        if b == b'\n' {
            *line += 1;
            *col_in_line = 0;
        } else if b & 0b1100_0000 != 0b1000_0000 {
            *col_in_line += 1;
        }
    }
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
///
/// 行列号由调用方（增量游标）给出 —— 这里不再自己扫文本（见 [`extract_skipping_guarded`] 的说明）。
fn build(inner: &str, embed: bool, line: u32, col: u32) -> Option<Link> {
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
