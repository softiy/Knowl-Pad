//! 高级搜索语法解析的测试（FR-SEARCH-20~27）。
//!
//! 判据都打在**解析结果**上：每条语法各自的取值、与全文关键词的混用、
//! 以及语法错误必须变成 E_SEARCH_SYNTAX（而不是被静默吞掉）。

use super::*;

fn filters(query: &str) -> SearchFilters {
    extract_filters(query).expect("正常查询不该报错").0
}

fn rest(query: &str) -> String {
    extract_filters(query).expect("正常查询不该报错").1
}

/// FR-SEARCH-20：path:folder/ 摘成路径前缀；尾斜杠与反斜杠都归一化掉（R-09）。
#[test]
fn path_prefix_is_extracted_and_normalized() {
    assert_eq!(
        filters("path:work/2026/ 报告").path_prefix,
        Some("work/2026".to_string())
    );
    assert_eq!(
        filters("path:work\\sub 报告").path_prefix,
        Some("work/sub".to_string()),
        "Windows 分隔符统一为 /"
    );
    assert_eq!(rest("path:work/2026/ 报告"), "报告", "过滤条件不进全文");
}

/// FR-SEARCH-21：file:名称 摘成文件名过滤；**不参与全文**。
#[test]
fn file_name_is_extracted_and_not_a_fulltext_word() {
    let f = filters("file:季度报告 内容");
    assert_eq!(f.file_name, Some("季度报告".to_string()));
    assert_eq!(rest("file:季度报告 内容"), "内容");
}

/// FR-SEARCH-22：tag:#标签 去 # 并转小写（MD-TAG-05 的 norm 口径）；多个 tag: 是 AND。
#[test]
fn tags_are_normalized_to_norm_form() {
    let f = filters("tag:#工作/2026 报告");
    assert_eq!(f.tags, vec!["工作/2026".to_string()]);
    assert_eq!(filters("tag:Work").tags, vec!["work".to_string()]);
    assert_eq!(
        filters("tag:#工作 tag:#2026").tags,
        vec!["工作".to_string(), "2026".to_string()],
        "多个 tag: 都保留（storage 侧按 AND 组合）"
    );
    assert_eq!(
        filters("tag:#工作/").tags,
        vec!["工作".to_string()],
        "折叠尾斜杠"
    );
}

/// FR-SEARCH-23：line:(...) 组内按空白拆词，允许带引号的精确短语。
#[test]
fn line_group_terms_are_split() {
    let f = filters("line:(季度 报告) 结尾");
    assert_eq!(f.line_terms, vec!["季度".to_string(), "报告".to_string()]);
    assert_eq!(rest("line:(季度 报告) 结尾"), "结尾", "组内词不进剩余全文");
    let quoted = filters("line:(\"季度报告\" 草稿)");
    assert_eq!(
        quoted.line_terms,
        vec!["季度报告".to_string(), "草稿".to_string()]
    );
}

/// FR-SEARCH-24：双引号短语整体保留（带引号交回全文层，句内空白不被拆开）。
#[test]
fn quoted_phrases_survive_filter_extraction() {
    let (f, text) = extract_filters("tag:#工作 \"季度 报告\" 尾巴").expect("解析");
    assert_eq!(f.tags, vec!["工作".to_string()]);
    assert_eq!(text, "\"季度 报告\" 尾巴");
    let (phrases, remainder) = extract_quoted(&text).expect("解析");
    assert_eq!(phrases, vec!["季度 报告".to_string()], "短语不被拆词");
    assert_eq!(remainder, "尾巴");
}

/// FR-SEARCH-26：四条语法 + 排除项混用时互不干扰。
#[test]
fn all_constructs_mix() {
    let (f, text) = extract_filters("path:a/ file:b tag:#c line:(d e) \"f g\" -h").expect("解析");
    assert_eq!(f.path_prefix, Some("a".to_string()));
    assert_eq!(f.file_name, Some("b".to_string()));
    assert_eq!(f.tags, vec!["c".to_string()]);
    assert_eq!(f.line_terms, vec!["d".to_string(), "e".to_string()]);
    assert_eq!(text, "\"f g\" -h", "排除项留在全文层处理");
}

/// 引号里的 path: 是**普通短语**，不是过滤条件（否则用户没法搜这个字面量）。
#[test]
fn filter_lookalike_inside_quotes_is_not_a_filter() {
    let f = filters("\"path:work\" 报告");
    assert!(f.is_empty(), "{f:?}");
    assert_eq!(rest("\"path:work\" 报告"), "\"path:work\" 报告");
}

/// **2026-10-15 项目所有者裁定**（口径已落 PRD §5.3.4.3）：运算符**大小写不敏感**。
#[test]
fn operators_are_case_insensitive() {
    assert_eq!(filters("Tag:#工作").tags, vec!["工作".to_string()]);
    assert_eq!(filters("TAG:#工作").tags, vec!["工作".to_string()]);
    assert_eq!(filters("tAg:#工作").tags, vec!["工作".to_string()]);
    assert_eq!(filters("PATH:work/").path_prefix, Some("work".to_string()));
    assert_eq!(filters("File:报告").file_name, Some("报告".to_string()));
    assert_eq!(
        filters("Line:(甲 乙)").line_terms,
        vec!["甲".to_string(), "乙".to_string()],
        "含括号的 line: 同样大小写不敏感"
    );
    assert_eq!(rest("Tag:#工作 内容"), "内容", "大写运算符同样不进全文");
}

/// 大小写放宽**不改变**「前缀必须位于词首且紧跟冒号」这条约束。
#[test]
fn operators_must_start_the_word_and_touch_the_colon() {
    assert!(filters("xTag:#工作").tags.is_empty(), "前缀必须在词首");
    assert_eq!(
        rest("xTag:#工作"),
        "xTag:#工作",
        "词中出现的同形串按普通检索词处理"
    );
    assert!(filters("tag :#工作").tags.is_empty(), "冒号前不能有空格");
    assert!(filters("tag#工作").tags.is_empty(), "缺冒号不是运算符");
}

/// `line:` 没有紧跟括号是**残缺语法**：必须报 E_SEARCH_SYNTAX，
/// 不得当普通检索词悄悄吞掉（PRD §5.3.4.3 的空取值规则）。
#[test]
fn line_without_paren_is_a_syntax_error() {
    for query in ["line:", "line: 词", "Line:", "LINE:甲"] {
        let err = extract_filters(query).expect_err("必须报语法错误");
        assert_eq!(err.code(), "E_SEARCH_SYNTAX", "查询：{query}");
        assert!(err.to_string().contains("括号"), "查询：{query}：{err}");
    }
}

/// 空查询与「无过滤条件」。
#[test]
fn empty_query_yields_no_filters() {
    let (f, text) = extract_filters("   ").expect("解析");
    assert!(f.is_empty());
    assert_eq!(text, "");
    assert_eq!(extract_quoted("").expect("解析"), (vec![], String::new()));
}

/// AC-SEARCH-05：未闭合引号必须报 E_SEARCH_SYNTAX，且**文案不含用户输入**。
#[test]
fn unclosed_quote_reports_syntax_error_without_echoing_input() {
    let err = extract_filters("\"未闭合的短语").expect_err("必须报语法错误");
    assert_eq!(err.code(), "E_SEARCH_SYNTAX");
    assert!(err.to_string().contains("双引号未闭合"), "{err}");
    assert!(!err.to_string().contains("未闭合的短语"), "{err}");
    assert_eq!(
        extract_quoted("\"半截").expect_err("必须报语法错误").code(),
        "E_SEARCH_SYNTAX"
    );
}

/// line:( 的括号未闭合同样是语法错误（FR-SEARCH-27：不得静默）。
#[test]
fn unclosed_line_group_reports_syntax_error() {
    let err = extract_filters("line:(季度 报告").expect_err("必须报语法错误");
    assert_eq!(err.code(), "E_SEARCH_SYNTAX");
    assert!(err.to_string().contains("括号未闭合"), "{err}");
}

/// 空取值规则（**2026-10-15 裁定**，口径已落 PRD §5.3.4.3）：
/// `path:/` 合法（= Vault 根）；`file:` / `tag:` / `line:()` 空取值报 E_SEARCH_SYNTAX。
///
/// 报错的理由：只有键没有值一定是笔误，静默忽略会让用户以为过滤已经生效。
#[test]
fn empty_filter_values_are_syntax_errors() {
    for query in ["file:", "tag:", "tag:#", "line:()", "FILE:", "Tag:#"] {
        let err = extract_filters(query).expect_err("必须报语法错误");
        assert_eq!(err.code(), "E_SEARCH_SYNTAX", "查询：{query}");
        assert!(err.to_string().contains("缺少取值"), "查询：{query}");
    }
    // path: 的空值等价于「Vault 根」= 不加路径限制 ⇒ **合法**，只是没有过滤效果
    for query in ["path:/ 报告", "path: 报告", "PATH:/ 报告"] {
        let (f, text) = extract_filters(query).expect("path: 的空值合法");
        assert!(f.path_prefix.is_none(), "查询：{query}");
        assert_eq!(text, "报告", "查询：{query}");
    }
}

/// 语法错误的文案里**只含固定诊断**，不回显用户输入（SEC-09 / R-12 的同一纪律）。
#[test]
fn syntax_error_messages_carry_no_user_data() {
    let err = extract_filters("line:(秘密值").expect_err("必须报错");
    assert!(!err.to_string().contains("秘密值"), "{err}");
    let err = extract_filters("\"机密短语").expect_err("必须报错");
    assert!(!err.to_string().contains("机密短语"), "{err}");
}
