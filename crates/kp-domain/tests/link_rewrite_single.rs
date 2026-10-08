//! rewrite_occurrence_at 的判据（FR-LINK-22 / AC-LINK-05）。
//!
//! 关键性质：**只动指定位置的那一处** —— 同一文件里其它指向同名笔记的链接必须原样保留，
//! 否则"用户为一条歧义链接指定目标"会顺带改掉别的引用。

use kp_domain::link_rewrite_single::rewrite_occurrence_at;

#[test]
fn rewrites_only_the_occurrence_at_the_given_position() {
    // 第 1 行一处、第 3 行一处，词干相同
    let content = "见 [[note]] 与 [[note|别名]]\n\n再见 [[note]]\n";
    // 精确命中第 3 行第 4 列（"再见 " 之后）的那一处
    let (out, hits) = rewrite_occurrence_at(content, 3, 4, "folder2/note").expect("应命中一处");
    assert_eq!(hits, 1);
    assert!(
        out.contains("见 [[note]] 与 [[note|别名]]"),
        "第 1 行的两处必须原样保留：{out}"
    );
    assert!(
        out.contains("再见 [[folder2/note]]"),
        "只改第 3 行那处：{out}"
    );
}

#[test]
fn keeps_alias_and_anchor_when_resolving() {
    let (out, _) =
        rewrite_occurrence_at("见 [[note#标题|别名]]\n", 1, 3, "f/note").expect("应命中");
    assert_eq!(out, "见 [[f/note#标题|别名]]\n", "锚点与别名都要保留");
}

#[test]
fn returns_none_when_position_has_no_link() {
    assert!(rewrite_occurrence_at("没有任何链接\n", 1, 1, "x").is_none());
    assert!(
        rewrite_occurrence_at("见 [[note]]\n", 9, 1, "x").is_none(),
        "行号越界应返回 None"
    );
}
