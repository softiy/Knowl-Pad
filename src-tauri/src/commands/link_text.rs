//! 链接在原文里的字符数（供反链片段的高亮区间使用）。
//!
//! 为什么单独一个模块：这个纯函数原先内联在 `link.rs`，抽出后无论放回 `link.rs` 还是放进
//! `link_view.rs`，都会把那个文件推过 CODE-11 的 200 行阈值 —— 与其在两者之间来回搬运
//! （每搬一次只是"换一个文件超限"），不如给它一个语义明确的小模块。

/// 链接在**原文里的字符数**，供反链片段的高亮区间使用（审计 L1）。
///
/// 此前调用点固定按 `[[目标]]` 估算（target + 4），于是带**别名/锚点/嵌入**的链接高亮区间
/// 一律偏短 —— 例如 `[[目标|别名]]` 会少掉 `|别名` 那一段。反链片段已经在用这个值，
/// 所以它不是"接线才错"，而是**现在就是错的**。
///
/// 按各部件精确累加（**字符数**而非字节，与 extract_snippet 的口径一致）：
/// `[[目标]]` = target + 4；带锚点再 + 1 + 锚点；带别名再 + 1 + 别名；嵌入再 + 1（叹号）。
///
/// 抽成纯函数是为了可回归：命令层无法单测（DEBT-14），纯函数可以。
pub(crate) fn link_text_len(
    target: &str,
    anchor: Option<&str>,
    alias: Option<&str>,
    is_embed: bool,
) -> usize {
    let mut len = target.chars().count() + 4;
    if let Some(anchor) = anchor {
        len += 1 + anchor.chars().count();
    }
    if let Some(alias) = alias {
        len += 1 + alias.chars().count();
    }
    if is_embed {
        len += 1;
    }
    len
}
