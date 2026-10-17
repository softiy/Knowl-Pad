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
//! 块 ID（MD-BID-01~03）、plain_text 的完整清洗。
//!
//! **超时保护（SEC-03 / DEBT-32）**：本模块提供 [`parse_with_guard`]（协作式中止，检查点见其文档），
//! 由调用方给出"是否该停"的判据（索引路径是 10s 截止时间）；**>5MB 跳过**由调用方负责（扫描侧）。

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

/// 解析被预算中止（SEC-03 的单文件超时；DEBT-32）。
///
/// **语义：中止不返回半成品。** 调用方拿到 `Err` 后必须跳过该文件并记警告
/// （PRD SEC-03：「超限跳过并记录警告」），绝不允许拿一个"解析到一半"的结果去建索引 ——
/// 那会把"超时保护"变成"静默的内容丢失"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseAborted;

/// 纯文本清洗的中止检查步长（字节）。
///
/// 取 64 KiB：5MB 上限下最多约 80 次检查（每次读一次时钟，纳秒级），换来"清洗大文件时也能及时停下"。
const ABORT_CHECK_STRIDE: usize = 64 * 1024;

/// 解析一篇笔记，并在 `abort()` 返回 true 时**立刻中止**（SEC-03 / DEBT-32）。
///
/// 为什么是协作式中止、而不是"另起线程 + join 超时"：
/// ① 零新依赖，也不需要 unsafe 去强杀线程；② 线程无法被安全中断 —— 超时后那条线程仍会继续
/// 吃掉 CPU，而我们已经在写"该文件跳过"的结论，等于"看着有超时、其实没有"；
/// ③ 协作式检查是确定性的，可以被用例钉住（见 `guard_aborts_without_partial_result`）。
///
/// 检查点：进入扫描前、每个扫描阶段之间、纯文本清洗每 [`ABORT_CHECK_STRIDE`] 字节。
/// 因此**超出预算的量级是「一个扫描阶段的最坏耗时」**，而不是整篇解析：所有扫描器都是线性时间
/// （无回溯、无正则），输入另有 5MB 上限（SEC-11）。
pub fn parse_with_guard(
    bytes: &[u8],
    abort: &dyn Fn() -> bool,
) -> Result<ParsedNote, ParseAborted> {
    parse_inner(bytes, abort)
}

/// 解析一篇笔记。**永不失败**：非 UTF-8 输入按有损转换继续并记警告。
///
/// 无预算版本：`abort` 恒 false，行为与加超时保护之前**逐字节一致**
/// （用例 `parse_without_guard_is_unchanged` 钉住）。生产索引路径用 [`parse_with_guard`]。
pub fn parse(bytes: &[u8]) -> ParsedNote {
    // never_abort 恒 false ⇒ 这里不可能是 Err。仍写 match 而不 unwrap：解析在索引批事务里被调用，
    // 留一个 panic 点等于把"理论上不可达"变成一次失败的写任务。
    match parse_inner(bytes, &never_abort) {
        Ok(note) => note,
        Err(_) => unreachable_note(),
    }
}

fn never_abort() -> bool {
    false
}

/// `parse` 的"不可达"回退：给出**空结果 + 一条警告**，而不是 panic（见 `parse` 的注释）。
fn unreachable_note() -> ParsedNote {
    ParsedNote {
        frontmatter: None,
        tags: Vec::new(),
        headings: Vec::new(),
        block_ids: Vec::new(),
        links: Vec::new(),
        warnings: vec!["PARSE_ABORTED：解析被中止（这是不应到达的路径，请上报）".to_string()],
        plain_text: String::new(),
        has_bom: false,
        line_ending: LineEnding::Lf,
    }
}

/// 中止检查点（位置与理由见 [`parse_with_guard`]）。
fn check(abort: &dyn Fn() -> bool) -> Result<(), ParseAborted> {
    if abort() {
        Err(ParseAborted)
    } else {
        Ok(())
    }
}

/// 解析实现：`abort` 为真时立刻返回 `Err`，**不产出任何部分结果**。
fn parse_inner(bytes: &[u8], abort: &dyn Fn() -> bool) -> Result<ParsedNote, ParseAborted> {
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
    check(abort)?;
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
    check(abort)?;
    let code = CodeRanges::scan(&text);
    check(abort)?;
    let links = wikilink::extract_skipping(&text, &code, skip.clone());
    check(abort)?;
    let headings = heading::extract(&text, &code, skip.clone());
    check(abort)?;
    let block_ids = block_id::extract(&text, &code, skip.clone());
    check(abort)?;
    let mut tags =
        tag::from_frontmatter(fm.data.as_ref().map(|f| f.tags.as_slice()).unwrap_or(&[]));
    tags.extend(tag::extract(&text, &code, skip.clone()));
    check(abort)?;
    let plain_text = plain_text(&text, &code, skip, abort)?;
    Ok(ParsedNote {
        frontmatter: fm.data,
        tags,
        headings,
        block_ids,
        links,
        warnings,
        plain_text,
        has_bom,
        line_ending,
    })
}

/// 去掉代码区间与 frontmatter 区间后的正文（MD-FM-06；完整清洗随后续切片完善）。
///
/// 这是全文件最长的一条循环，因此按 [`ABORT_CHECK_STRIDE`] 分块检查中止（见 `parse_with_guard`）。
fn plain_text(
    text: &str,
    code: &CodeRanges,
    skip: Option<std::ops::Range<usize>>,
    abort: &dyn Fn() -> bool,
) -> Result<String, ParseAborted> {
    let mut raw = String::with_capacity(text.len());
    for (i, ch) in text.char_indices() {
        if i % ABORT_CHECK_STRIDE == 0 {
            check(abort)?;
        }
        let skipped = skip.as_ref().is_some_and(|r| r.contains(&i));
        if !skipped && !code.contains(i) {
            raw.push(ch);
        }
    }
    Ok(clean_markdown(&raw))
}

/// 全文索引用的清洗：`[[目标|别名]]` → `别名`、`[[目标]]` → `目标`，再剥离行内标记。
/// 目的：让 FTS 的 `plain_text` 是「可被检索的自然语言」，而不是 Markdown 源码。
fn clean_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("[[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find("]]") else {
            out.push_str(&rest[i..]);
            rest = "";
            break;
        };
        let inner = &after[..j];
        out.push_str(inner.split('|').next_back().unwrap_or(inner));
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    heading::strip_markers(&out)
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
    #[test]
    fn plain_text_unwraps_wikilinks_and_strips_markers() {
        let note = parse("见 [[目标|别名]] 与 [[纯目标]]，另有 **粗体** 与 `代码`\n".as_bytes());
        assert!(
            note.plain_text.contains("别名"),
            "带别名时取别名：{}",
            note.plain_text
        );
        assert!(
            note.plain_text.contains("纯目标"),
            "无别名时取目标：{}",
            note.plain_text
        );
        assert!(note.plain_text.contains("粗体"));
        assert!(!note.plain_text.contains("**"), "行内标记应被剥离");
        assert!(!note.plain_text.contains("[[") && !note.plain_text.contains("]]"));
    }

    /// **DEBT-32 / SEC-03 的判据**：判据说停时必须中止，且**不返回半成品**（类型上就是 `Err`）。
    ///
    /// 用"第 3 次检查时中止"的闭包而不是墙钟：真实超时是时间事件，用它写用例必然 flaky；
    /// 而协作式中止的契约本身是"每到一个检查点问一次判据，判据说停就停" —— 这正是本用例钉的东西。
    #[test]
    fn guard_aborts_without_partial_result() {
        let text = "---\ntags: [a/b]\n---\n# 标题\n正文 [[链接]] 与 #标签\n";
        let calls = std::cell::Cell::new(0u32);
        let abort = || {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        };
        let outcome = parse_with_guard(text.as_bytes(), &abort);
        assert!(outcome.is_err(), "判据说停时必须中止");
        assert!(
            calls.get() >= 3,
            "中止判据必须真的被问到（实际 {} 次）",
            calls.get()
        );
    }

    /// 无预算路径（`parse`）与"永不中止的预算"必须给出**完全相同**的结果 —— 正常路径行为不变。
    #[test]
    fn parse_without_guard_is_unchanged() {
        let text = "---\ntags: [甲/乙]\n---\n# 标题\n正文 [[目标|别名]] #丙 ^bid\n```\ncode\n```\n";
        let plain = parse(text.as_bytes());
        let guarded = parse_with_guard(text.as_bytes(), &never_abort).expect("永不中止");
        assert_eq!(plain, guarded, "无预算解析的结果必须逐字段相同");
    }

    /// 超大输入 + 已耗尽的预算 ⇒ 立刻中止（可复现，不依赖真的等 10s）。
    ///
    /// 同一份输入在"永不中止"下必须能正常解析完 —— 否则上一条断言可能只是因为输入非法而"通过"。
    #[test]
    fn oversized_input_aborts_when_budget_is_already_spent() {
        // ≈0.6MB、5 万行：远超常规笔记，足以代表"超大输入"。
        //
        // **刻意不放 wikilink**：`wikilink::line_col` 是"每个链接都从文件头扫一遍"的 O(位置) 实现，
        // "每行一个 [[链接]]"会退化成 O(n²)（实测 1MB / 5 万链接要烧掉数百秒 CPU）—— 那是另一个
        // 已登记的独立缺陷（技术方案 §13.2 的 DEBT-42），不该让本用例替它背锅。
        let big = "正文内容行，用于构造超大输入。\n".repeat(50_000);
        assert!(
            parse_with_guard(big.as_bytes(), &|| true).is_err(),
            "预算已耗尽时必须立刻中止"
        );
        let ok = parse_with_guard(big.as_bytes(), &never_abort).expect("无预算应能解析");
        assert!(
            ok.links.is_empty() && ok.tags.is_empty(),
            "本输入不含链接与标签"
        );
        assert!(
            ok.plain_text.len() >= 50_000,
            "大输入应被完整解析：{}",
            ok.plain_text.len()
        );
    }

    #[test]
    fn index_text_from_plain_text_contains_chinese_words() {
        let note = parse("知识图谱的力导向布局\n".as_bytes());
        let indexed = crate::tokenize::for_index(&note.plain_text);
        assert!(indexed.contains("知识"), "plain_text 应能被分词：{indexed}");
    }
}
