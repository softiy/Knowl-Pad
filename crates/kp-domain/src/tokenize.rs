//! 中文分词（技术方案 §5.2；PRD FTS-01~FTS-05）。
//!
//! 关键约束：
//! - **非 HMM**（`cut(text, false)`）：HMM 的概率输出可能随版本/词典微变，导致同一文本两次索引结果不同，
//!   从而破坏 **AC-REL-03**（删库重建后逐项一致）；
//! - 词典**全量内置**（jieba-rs 自带，不联网、不读外部文件）；
//! - 索引侧把分词结果以**单空格连接**写入 `note_fts.plain_text`（FTS-01），查询侧同样先分词（FTS-02）。

use std::sync::{Arc, OnceLock};

use jieba_rs::Jieba;

/// 分词器版本（进索引签名；**升级 jieba-rs 时必须同步修改**）。
///
/// 单一来源在 `tokenizer_meta`：`build.rs` 也用 `#[path]` 引入同一个文件算词典指纹，
/// 避免"库里的版本"与"指纹清单里的版本"两处漂移。
pub use crate::tokenizer_meta::TOKENIZER_VERSION;

/// 词典指纹（**构建期**由 `build.rs` 算出；口径见 `tokenizer_meta` 的模块头）。
///
/// 进索引签名：值变了 ⇒ 既有索引判废重建（技术方案 §4.6）。
pub const fn dict_hash() -> &'static str {
    env!("KP_TOKENIZER_DICT_HASH")
}

/// 指纹对应的**规范清单**（构建期写入；用于自检与诊断，不参与签名）。
pub const fn dict_manifest() -> &'static str {
    env!("KP_TOKENIZER_DICT_MANIFEST")
}

static JIEBA: OnceLock<Arc<Jieba>> = OnceLock::new();

/// 全局单例（词典只加载一次；AppState 亦复用同一实例）。
pub fn jieba() -> &'static Arc<Jieba> {
    JIEBA.get_or_init(|| Arc::new(Jieba::new()))
}

/// **预热**：把词典加载的一次性成本（实测 ≈0.5 s，见 NFR-PERF-03 分项计时）提前到调用点。
///
/// 为什么需要它：这笔成本原本落在**第一次分词**上 —— 也就是第一次索引或第一次搜索的耗时里。
/// 应用启动时在后台线程调一次，用户触发索引/搜索时词典已经就绪。
///
/// **不改变行为**：加载的是同一份内置词典（同一 `OnceLock`），分词结果逐字节不变
/// （见 `warm_up_does_not_change_tokenization` 的断言）。
pub fn warm_up() {
    let _ = jieba();
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

    /// 预热**不改变**分词结果：它只是把词典加载提前（NFR-PERF-03 的 0.5 s 一次性成本），
    /// 索引与查询两侧的输出必须与预热前逐字节一致。
    #[test]
    fn warm_up_does_not_change_tokenization() {
        let text = "知识图谱的力导向布局 markdown FTS5";
        let before = (for_index(text), for_query(text));
        warm_up();
        let after = (for_index(text), for_query(text));
        assert_eq!(before, after, "预热前后必须逐字节一致");
        warm_up(); // 幂等：再调一次也不变
        assert_eq!(before, (for_index(text), for_query(text)));
        assert!(jieba().cut("知识图谱", false).len() > 1, "预热后分词器可用");
    }

    #[test]
    fn query_side_matches_index_side_vocabulary() {
        let indexed = for_index("知识图谱的力导向布局");
        let q = for_query("力导向布局");
        assert!(!q.is_empty());
        assert!(indexed.contains(&q[0]), "查询词应能在索引文本中找到");
    }
}
