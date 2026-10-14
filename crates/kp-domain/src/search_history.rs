//! 搜索历史的**语义**（FR-SEARCH-10：保留最近 20 条查询，可一键复用、可清空）。
//!
//! 拆分理由：「记录什么、怎么去重、截断到几条」是**行为**（可脱离数据库逐条断言），
//! 而「存在哪张表、用什么编码」是存储层的事（src-tauri/src/storage/search_history.rs）。
//! kp-domain 不依赖 serde / rusqlite，因此这里只做纯字符串列表运算。

/// FR-SEARCH-10 的条数上限。
pub const HISTORY_LIMIT: usize = 20;

/// 把一次查询并入历史：**去重（同一条移到最前）+ 截断到上限**。
///
/// - 空白查询不入历史：FR-SEARCH-08 要求输入停顿即触发查询，若把空串也记进去，
///   用户每清空一次输入框就会留一条空记录；
/// - 去重按**精确匹配**（不做大小写折叠）：历史是用户自己的输入，"Rust" 与 "rust"
///   在界面上是两条不同的可复用查询，替用户合并属于擅自改写输入。
pub fn record(history: &[String], query: &str) -> Vec<String> {
    record_with_limit(history, query, HISTORY_LIMIT)
}

/// 同 record，但可显式给定上限（供测试与将来的「历史条数」设置项使用）。
pub fn record_with_limit(history: &[String], query: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        // 上限为 0 表示「不保留历史」（设置项可能这么配），此时连既有记录一并清空
        return Vec::new();
    }
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return history.to_vec();
    }
    let mut out: Vec<String> = Vec::with_capacity(history.len().min(limit) + 1);
    out.push(trimmed.to_string());
    for item in history {
        if item != trimmed && out.len() < limit {
            out.push(item.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn limit_matches_fr_search_10() {
        assert_eq!(HISTORY_LIMIT, 20, "FR-SEARCH-10：保留最近 20 条查询");
    }

    #[test]
    fn newest_query_goes_first() {
        let got = record(&list(&["旧一", "旧二"]), "新");
        assert_eq!(got, list(&["新", "旧一", "旧二"]), "新查询在最前");
    }

    #[test]
    fn repeated_query_moves_to_front_without_duplicating() {
        let got = record(&list(&["甲", "乙", "丙"]), "乙");
        assert_eq!(got, list(&["乙", "甲", "丙"]), "重复查询只留一条并提前");
        assert_eq!(got.len(), 3, "去重后条数不变");
    }

    #[test]
    fn blank_query_is_not_recorded() {
        let before = list(&["甲"]);
        assert_eq!(record(&before, "   "), before, "纯空白不入历史");
        assert_eq!(record(&before, ""), before, "空串不入历史");
        assert_eq!(record(&before, "  \t \n "), before, "制表/换行也不入历史");
        assert_eq!(
            record(&before, "  乙  "),
            list(&["乙", "甲"]),
            "前后空白被裁掉"
        );
    }

    #[test]
    fn history_is_capped_and_keeps_the_newest() {
        let full = list(&["q1", "q2", "q3", "q4"]);
        let got = record_with_limit(&full, "q0", 3);
        assert_eq!(got, list(&["q0", "q1", "q2"]), "超出上限时丢掉最旧的");
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn limit_zero_records_nothing() {
        assert!(record_with_limit(&list(&["甲"]), "乙", 0).is_empty());
    }

    #[test]
    fn case_differences_are_two_distinct_entries() {
        let got = record(&list(&["rust"]), "Rust");
        assert_eq!(got, list(&["Rust", "rust"]), "大小写不同视为两条");
    }
}
