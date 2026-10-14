//! 全文搜索的**命中定位**（FR-SEARCH-04/05）：把查询词在**原文**里定位出来，
//! 供「命中次数」与上下文片段使用。
//!
//! **为什么不用 FTS5 的 snippet() / highlight()**：它们作用在写入 note_fts 的 plain_text 上，
//! 而那是 **jieba 分词后用空格连接**的文本 —— 直接返回会带空格、与原文不逐字一致，
//! 高亮位置也会错位（技术方案 §5.2 的「高亮还原」一行明确要求：取命中 → 读原文 → 原文定位 → 截窗口）。

use crate::find_in_file::{find_all, site_at, FindOptions, MatchSite};

/// 一处命中（已合并重叠区间；行列口径同 MatchSite）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: u32,
    pub col: u32,
    pub len_chars: usize,
}

impl From<MatchSite> for Hit {
    fn from(m: MatchSite) -> Self {
        Self {
            start_byte: m.start_byte,
            end_byte: m.end_byte,
            line: m.line,
            col: m.col,
            len_chars: m.len_chars,
        }
    }
}

/// 合并重叠或相邻的字节区间（输入无需有序）。
fn merge_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (s, e) in spans {
        match out.last_mut() {
            // 相邻（s == 上一个 e）也算一次命中：「知识」+「图谱」在「知识图谱」里是一处，不是两处
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// 定位全部命中：逐词扫描（默认不区分大小写，与索引侧 FTS5 的行为一致）→ 合并 → 按位置升序。
///
/// 合并的语义价值：查询「知识 图谱」命中「知识图谱」时，**命中次数是 1 而不是 2** ——
/// 否则 FR-SEARCH-05 的「命中次数」会随分词粒度虚高，用户无法解释这个数字。
pub fn locate(content: &str, terms: &[String]) -> Vec<Hit> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for term in terms {
        for m in find_all(content, term, FindOptions::default()) {
            spans.push((m.start_byte, m.end_byte));
        }
    }
    merge_spans(spans)
        .into_iter()
        .filter_map(|(s, e)| site_at(content, s, e))
        .map(Hit::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn locates_each_occurrence_in_original_text() {
        let doc = "第一行\n力导向布局 很好\n力导向布局 又一次";
        let hits = locate(doc, &terms(&["力导向布局"]));
        assert_eq!(hits.len(), 2, "两处命中");
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[1].line, 3);
        assert_eq!(hits[0].len_chars, 5);
        assert_eq!(&doc[hits[1].start_byte..hits[1].end_byte], "力导向布局");
    }

    #[test]
    fn adjacent_terms_merge_into_one_hit() {
        // 「知识图谱」被 jieba 切成「知识」「图谱」，但原文里是**一处**连续命中
        let hits = locate("知识图谱的力导向布局", &terms(&["知识", "图谱"]));
        assert_eq!(hits.len(), 1, "相邻命中合并为一次");
        assert_eq!(hits[0].len_chars, 4, "合并后覆盖「知识图谱」");
        let layout = locate("知识图谱的力导向布局", &terms(&["力导向布局"]));
        assert_eq!(layout.len(), 1);
    }

    #[test]
    fn overlapping_terms_merge_and_are_counted_once() {
        let hits = locate("abcabc", &terms(&["abc", "bca"]));
        assert_eq!(hits.len(), 1, "重叠区间只算一次");
        assert_eq!(hits[0].len_chars, 6);
    }

    #[test]
    fn no_terms_or_no_match_yields_empty() {
        assert!(locate("任意正文", &[]).is_empty());
        assert!(locate("任意正文", &terms(&["不存在"])).is_empty());
        assert!(
            locate("任意正文", &terms(&[""])).is_empty(),
            "空词不得命中全文"
        );
    }

    #[test]
    fn matching_is_case_insensitive_like_the_index() {
        let hits = locate("Rust 与 rust", &terms(&["rust"]));
        assert_eq!(hits.len(), 2, "与 unicode61 索引侧的 ASCII 行为一致");
    }

    #[test]
    fn hits_are_ordered_by_position() {
        let doc = "乙 甲 甲 乙";
        let hits = locate(doc, &terms(&["乙", "甲"]));
        assert_eq!(hits.len(), 4);
        let mut sorted = hits.clone();
        sorted.sort_by_key(|h| h.start_byte);
        assert_eq!(hits, sorted, "输出必须按位置升序");
    }
}
