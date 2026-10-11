//! 逐处差异的判据（技术方案 §6.1.1 的"每一处原文 → 新文"）。

use kp_domain::link_rewrite_diff::occurrence_diffs;

#[test]
fn reports_the_link_span_not_the_whole_line() {
    let old = "第一行\n见 [[A]] 与别的字\n";
    let new = "第一行\n见 [[B]] 与别的字\n";
    let diffs = occurrence_diffs(old, new, 10);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].line, 2, "行号 1-based");
    assert_eq!(diffs[0].before, "[[A]]", "只给出变化的那一段，而不是整行");
    assert_eq!(diffs[0].after, "[[B]]");
}

#[test]
fn keeps_alias_and_anchor_outside_the_changed_span() {
    let old = "见 [[A|别名]]\n";
    let new = "见 [[B|别名]]\n";
    let d = &occurrence_diffs(old, new, 10)[0];
    assert_eq!(d.before, "[[A|别名]]");
    assert_eq!(d.after, "[[B|别名]]");
}

#[test]
fn handles_multiple_lines_and_reports_each_once() {
    let old = "[[A]]\n不变\n[[A]] 两次\n";
    let new = "[[B]]\n不变\n[[B]] 两次\n";
    let diffs = occurrence_diffs(old, new, 10);
    assert_eq!(diffs.len(), 2, "两处各一条，相同行不报");
    assert_eq!(diffs.iter().map(|d| d.line).collect::<Vec<_>>(), vec![1, 3]);
}

#[test]
fn respects_max_and_zero() {
    let old = "[[A]]\n[[A]]\n[[A]]\n";
    let new = "[[B]]\n[[B]]\n[[B]]\n";
    assert_eq!(occurrence_diffs(old, new, 2).len(), 2, "受 max 限制");
    assert!(
        occurrence_diffs(old, new, 0).is_empty(),
        "max=0 时不做任何事"
    );
    assert_eq!(occurrence_diffs(old, new, 99).len(), 3);
}

#[test]
fn identical_input_yields_no_diff() {
    assert!(occurrence_diffs("一样\n", "一样\n", 10).is_empty());
}

#[test]
fn trailing_line_added_or_removed_is_reported() {
    let d = occurrence_diffs("甲\n", "甲\n乙\n", 10);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].line, 2);
    assert_eq!(d[0].before, "");
    assert_eq!(d[0].after, "乙");
}
