//! 中文分词（技术方案 §5.2；PRD FTS-01~FTS-05）。
//!
//! 关键约束：
//! - **非 HMM**（`cut(text, false)`）：HMM 的概率输出可能随版本/词典微变，导致同一文本两次索引结果不同，
//!   从而破坏 **AC-REL-03**（删库重建后逐项一致）；
//! - 词典**全量内置**（jieba-rs 自带，不联网、不读外部文件）；
//! - 索引侧把分词结果以**单空格连接**写入 `note_fts.plain_text`（FTS-01），查询侧同样先分词（FTS-02）。

use std::sync::{Arc, OnceLock};

use jieba_rs::Jieba;

/// 分词器版本（进索引签名；**升级 jieba-rs 时必须同步修改**，否则签名不变、不会触发重建）。
pub const TOKENIZER_VERSION: &str = "0.11.0";

static JIEBA: OnceLock<Arc<Jieba>> = OnceLock::new();

/// 全局单例（词典只加载一次；AppState 亦复用同一实例）。
pub fn jieba() -> &'static Arc<Jieba> {
    JIEBA.get_or_init(|| Arc::new(Jieba::new()))
}

/// 是否保留为索引 token：过滤纯空白与纯标点（中英文标点都不进 FTS）。
fn keep(token: &str) -> bool {
    token
        .chars()
        .any(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// 索引侧分词（FTS-01）：非 HMM → 过滤 → 单空格连接。
pub fn for_index(text: &str) -> String {
    jieba()
        .cut(text, false)
        .into_iter()
        .map(|token| token.word.trim())
        .filter(|t| keep(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 查询侧分词（FTS-02）：保留顺序与重复，供调用方构造 MATCH。
pub fn for_query(text: &str) -> Vec<String> {
    jieba()
        .cut(text, false)
        .into_iter()
        .map(|token| token.word.trim())
        .filter(|t| keep(t))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ft01_index_text_is_space_joined_tokens() {
        let out = for_index("知识图谱的力导向布局");
        assert!(out.contains("知识"), "中文应被切分：{out}");
        assert!(!out.contains("  "), "不得有连续空格");
        assert!(!out.starts_with(' ') && !out.ends_with(' '));
    }

    #[test]
    fn ft03_ascii_words_are_not_split() {
        let out = for_index("markdown FTS5 test");
        assert!(out.contains("markdown"));
        assert!(out.contains("FTS5"));
    }

    #[test]
    fn punctuation_only_tokens_are_dropped() {
        let out = for_index("—— …… ！！！");
        assert!(out.is_empty(), "纯标点不应进入索引：{out}");
    }

    #[test]
    fn determinism_two_runs_are_identical() {
        // 非 HMM 的直接目的：同样输入必须得到同样输出（AC-REL-03 的前提）
        let text = "同一段中文文本用于验证分词的可重复性";
        assert_eq!(for_index(text), for_index(text));
    }

    #[test]
    fn query_side_matches_index_side_vocabulary() {
        let indexed = for_index("知识图谱的力导向布局");
        let q = for_query("力导向布局");
        assert!(!q.is_empty());
        assert!(indexed.contains(&q[0]), "查询词应能在索引文本中找到");
    }
}
