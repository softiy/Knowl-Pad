//! find_in_file 的用例（CODE-11：实现留在 find_in_file.rs，用例按 error_tests.rs 的先例独立成文件）。

use super::*;

const NONE: FindOptions = FindOptions {
    case_sensitive: false,
    whole_word: false,
};

#[test]
fn literal_search_is_case_insensitive_by_default() {
    let hits = find_all("Rust 与 rust 与 RUST", "rust", NONE);
    assert_eq!(hits.len(), 3, "默认不区分大小写");
    let strict = find_all(
        "Rust 与 rust",
        "rust",
        FindOptions {
            case_sensitive: true,
            whole_word: false,
        },
    );
    assert_eq!(strict.len(), 1, "区分大小写时只命中一处");
}

#[test]
fn line_and_char_column_are_1_based_and_count_chars() {
    // 第 2 行「见 图谱 一词」：「图谱」从第 3 个字符开始
    let doc = "第一行\n见 图谱 一词\n末行";
    let hits = find_all(doc, "图谱", NONE);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].col, 3, "列按字符算，不是字节");
    assert_eq!(hits[0].len_chars, 2);
    assert_eq!(&doc[hits[0].start_byte..hits[0].end_byte], "图谱");
}

#[test]
fn whole_word_filters_partial_matches() {
    let doc = "图谱 知识图谱 图";
    let loose = find_all(doc, "图", NONE);
    assert_eq!(loose.len(), 3, "普通查找命中所有出现");
    let whole = find_all(
        doc,
        "图",
        FindOptions {
            case_sensitive: false,
            whole_word: true,
        },
    );
    assert_eq!(whole.len(), 1, "全字匹配只剩独立的那一个");
    assert_eq!(whole[0].col, 9, "命中的是行尾独立的「图」（第 9 个字符）");
    let ascii = find_all(
        "cat category cat.",
        "cat",
        FindOptions {
            case_sensitive: false,
            whole_word: true,
        },
    );
    assert_eq!(ascii.len(), 2, "标点边界算词边界，category 不算");
}

#[test]
fn empty_query_or_longer_than_content_finds_nothing() {
    assert!(find_all("abc", "", NONE).is_empty(), "空查询不命中");
    assert!(find_all("abc", "abcd", NONE).is_empty(), "查询比正文长");
}

#[test]
fn matches_never_overlap() {
    let hits = find_all("aaaa", "aa", NONE);
    assert_eq!(hits.len(), 2, "查找全部按不重叠口径");
    assert_eq!(hits[0].start_byte, 0);
    assert_eq!(hits[1].start_byte, 2);
}

#[test]
fn replace_applies_all_targets_and_keeps_later_offsets_valid() {
    let doc = "甲 图谱 与 图谱 结尾";
    let hits = find_all(doc, "图谱", NONE);
    assert_eq!(hits.len(), 2);
    let targets: Vec<ReplaceTarget> = hits
        .iter()
        .map(|h| ReplaceTarget {
            start_byte: h.start_byte,
            end_byte: h.end_byte,
            expect: "图谱".to_string(),
        })
        .collect();
    let out = apply_replacements(doc, &targets, "知识图谱").expect("应可替换");
    assert_eq!(out, "甲 知识图谱 与 知识图谱 结尾");
}

#[test]
fn replace_rejects_stale_targets_without_touching_anything() {
    let doc = "图谱 在这里";
    let targets = vec![ReplaceTarget {
        start_byte: 0,
        end_byte: 6,
        expect: "图谱".to_string(),
    }];
    assert!(apply_replacements(doc, &targets, "x").is_ok());
    // 内容变了（同一区间不再是「图谱」）→ 必须拒绝，而不是照样改写
    let moved = "别的 在这里";
    let err = apply_replacements(moved, &targets, "x").expect_err("陈旧目标必须拒绝");
    assert_eq!(err.code(), "E_WRITE_CONFLICT");
}

#[test]
fn replace_rejects_overlapping_or_out_of_range_targets() {
    let doc = "abcdef";
    let overlapping = vec![
        ReplaceTarget {
            start_byte: 0,
            end_byte: 3,
            expect: "abc".to_string(),
        },
        ReplaceTarget {
            start_byte: 2,
            end_byte: 4,
            expect: "cd".to_string(),
        },
    ];
    assert!(
        apply_replacements(doc, &overlapping, "x").is_err(),
        "重叠必须拒绝"
    );
    let out_of_range = vec![ReplaceTarget {
        start_byte: 4,
        end_byte: 99,
        expect: "ef".to_string(),
    }];
    assert!(
        apply_replacements(doc, &out_of_range, "x").is_err(),
        "越界必须拒绝"
    );
    let not_boundary = vec![ReplaceTarget {
        start_byte: 1,
        end_byte: 2,
        expect: "b".to_string(),
    }];
    assert!(
        apply_replacements("中", &not_boundary, "x").is_err(),
        "非字符边界必须拒绝"
    );
}

#[test]
fn replace_with_no_targets_is_a_noop() {
    assert_eq!(
        apply_replacements("原文", &[], "x").expect("无目标"),
        "原文"
    );
}

#[test]
fn located_matches_carry_the_whole_line_without_carriage_return() {
    let doc = "第一行 图谱\r\n第二行 图谱 尾巴\r\n第三行";
    let hits = find_all_with_lines(doc, "图谱", NONE);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line_text, "第一行 图谱", "整行文本不含 \\r");
    assert_eq!(hits[1].line_text, "第二行 图谱 尾巴");
    assert_eq!(hits[1].site.line, 2);
    // 末行也要能取到（没有下一行的 \n 可切）
    let last = find_all_with_lines(doc, "第三行", NONE);
    assert_eq!(last[0].line_text, "第三行");
}

#[test]
fn site_at_rejects_invalid_ranges() {
    assert!(site_at("abc", 1, 1).is_none());
    assert!(site_at("abc", 0, 9).is_none());
    assert!(site_at("中", 1, 2).is_none(), "落在字符中间");
    assert!(site_at("abc", 0, 1).is_some());
}
