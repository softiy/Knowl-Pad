//! 搜索域命令的测试（判据都打在命令体上，DEBT-14 的做法：命令体可直接用真实临时库调用）。
//!
//! 覆盖 PRD §5.3.4 的 6 个命令，判据对着需求：AC-SEARCH-01（中文短语命中 + 片段高亮）、
//! FR-SEARCH-04/05（片段、命中次数合并）、FR-SEARCH-06（排序）、FR-SEARCH-10（历史）、
//! FR-SEARCH-30/31（文件内查找与替换）、FR-SEARCH-40/41（快速打开）。

use std::sync::Arc;

use crate::commands::search::{
    search_history_clear_impl, search_history_get_impl, FindOptionsArg, InFileArgs, PageArg,
    QuickOpenArgs, ReplaceArgs, ReplaceMatchArg, SearchFulltextArgs, SearchOptions,
    MAX_LINE_FILTER_CANDIDATES,
};
use crate::commands::search_file::{replace_in_file_impl, search_in_file_impl};
use crate::commands::search_index::{search_fulltext_impl, search_quick_open_impl};
use crate::commands::search_page::{
    guard_candidate_count, page_with_line_filter, PageWindow, ViewOptions,
};
use crate::commands::search_view::{InFileMatch, SearchSnippet};
use crate::state::AppState;
use crate::storage::search_query::SortOrder;

/// 建临时 Vault（真实文件）+ 索引库（真实全量索引）+ 全局库，返回 AppState。
fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, AppState) {
    let vault = tempfile::tempdir().expect("Vault 临时目录");
    let root = vault.path().to_path_buf();
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).expect("建目录");
        }
        std::fs::write(&path, content).expect("写文件");
    }
    let pool = crate::storage::index::open(&root).expect("建索引库");
    crate::index_engine::full_index(&pool, &root, |_, _| {}).expect("首次索引");
    let config = tempfile::tempdir().expect("配置目录");
    let global = crate::storage::global::open(config.path()).expect("建全局库");
    let state = AppState::new();
    state.set_root(Some(root));
    state.set_index_db(Some(Arc::new(pool)));
    state.set_global_db(Arc::new(global));
    (vault, state)
}

fn fulltext(query: &str) -> SearchFulltextArgs {
    SearchFulltextArgs {
        query: query.to_string(),
        options: SearchOptions::default(),
        page: PageArg::default(),
    }
}

/// 从片段中把高亮区间切出来（字符偏移，0 基、左闭右开）。
fn highlighted(view: &SearchSnippet) -> String {
    let line = view
        .lines
        .iter()
        .find(|l| l.line == view.highlight_line)
        .expect("高亮行必须在片段里");
    line.text
        .chars()
        .skip(view.highlight_start)
        .take(view.highlight_end - view.highlight_start)
        .collect()
}

/// AC-SEARCH-01 的命令侧：三篇含「知识图谱的力导向布局」的笔记全部命中，片段里该短语被点亮。
#[test]
fn fulltext_hits_chinese_phrase_and_highlights_it() {
    let (_v, state) = fixture(&[
        ("a.md", "知识图谱的力导向布局很关键\n"),
        ("b.md", "另一篇也讲知识图谱的力导向布局\n"),
        ("c.md", "无关内容\n"),
    ]);
    let page = search_fulltext_impl(&state, fulltext("力导向布局")).expect("搜索");
    assert_eq!(page.total, 2, "两篇命中");
    assert_eq!(page.items.len(), 2);
    for item in &page.items {
        assert_eq!(item.hit_count, 1, "每篇一处命中");
        let first = item.snippets.first().expect("应带片段（FR-SEARCH-04）");
        assert_eq!(
            highlighted(first),
            "力导向布局",
            "高亮区间必须正好覆盖命中短语：{first:?}"
        );
    }
}

/// FR-SEARCH-05：同一笔记多处命中合并为**一条**结果，命中次数与片段数都对得上。
#[test]
fn fulltext_merges_multiple_hits_in_one_note() {
    let (_v, state) = fixture(&[(
        "many.md",
        "力导向布局 第一处\n中间行\n力导向布局 第二处\n又一行\n力导向布局 第三处\n",
    )]);
    let page = search_fulltext_impl(&state, fulltext("力导向布局")).expect("搜索");
    assert_eq!(page.items.len(), 1, "同一笔记只出一条结果");
    let item = &page.items[0];
    assert_eq!(item.hit_count, 3, "命中次数 3");
    assert_eq!(
        item.snippets.len(),
        3,
        "三处各一个片段（FR-SEARCH-05 可展开）"
    );
    let lines: Vec<u32> = item.snippets.iter().map(|s| s.highlight_line).collect();
    assert_eq!(lines, vec![1, 3, 5], "片段按位置升序，行号 1 基");
}

/// 片段上下文行数来自 options（PRD §4.11.3 默认 2）；命中行必须在片段中。
#[test]
fn snippet_context_lines_follow_options() {
    let (_v, state) = fixture(&[("ctx.md", "一\n二\n力导向布局\n四\n五\n")]);
    let mut args = fulltext("力导向布局");
    args.options.context_lines = Some(0);
    let page = search_fulltext_impl(&state, args).expect("搜索");
    let snippet = page.items[0].snippets.first().expect("片段");
    assert_eq!(snippet.lines.len(), 1, "contextLines=0 时只给命中行");
    assert!(snippet.lines[0].is_hit_line);
    // 命中在第 3 行、默认前后各 2 行 → 第 1..5 行（PRD §4.11.3 的「片段上下文行数 2」）
    let page2 = search_fulltext_impl(&state, fulltext("力导向布局")).expect("搜索");
    let lines: Vec<u32> = page2.items[0].snippets[0]
        .lines
        .iter()
        .map(|l| l.line)
        .collect();
    assert_eq!(lines, vec![1, 2, 3, 4, 5], "默认上下文行数 = 2");
}

/// 分页（IPC-05）：total 是总数、hasMore 与 offset/limit 一致。
#[test]
fn fulltext_paginates_with_total_and_has_more() {
    let (_v, state) = fixture(&[
        ("p1.md", "图谱\n"),
        ("p2.md", "图谱\n"),
        ("p3.md", "图谱\n"),
    ]);
    let mut args = fulltext("图谱");
    args.options.sort = Some("path".to_string());
    args.page.limit = Some(2);
    let first = search_fulltext_impl(&state, args).expect("第一页");
    assert_eq!(first.total, 3);
    assert_eq!(first.items.len(), 2);
    assert!(first.has_more, "还有下一页");
    assert_eq!(first.offset, 0);
    assert_eq!(first.limit, 2);
    let mut second_args = fulltext("图谱");
    second_args.options.sort = Some("path".to_string());
    second_args.page = PageArg {
        offset: Some(2),
        limit: Some(2),
    };
    let second = search_fulltext_impl(&state, second_args).expect("第二页");
    assert_eq!(second.items.len(), 1);
    assert!(!second.has_more, "最后一页");
    assert_eq!(second.items[0].rel_path, "p3.md", "不重不漏");
}

/// 每页条数被夹到上限（前端传 10000 不该让一次响应把 IPC 撑爆，IPC-05）。
#[test]
fn page_limit_is_clamped_to_max() {
    let (_v, state) = fixture(&[("a.md", "图谱\n")]);
    let mut args = fulltext("图谱");
    args.page.limit = Some(10_000);
    let page = search_fulltext_impl(&state, args).expect("搜索");
    assert_eq!(page.limit, crate::commands::search::MAX_PAGE_LIMIT);
}

/// 空查询 = 没有查询：返回空页而**不是**语法错误（kp_domain::search 的约定）。
#[test]
fn empty_query_returns_empty_page() {
    let (_v, state) = fixture(&[("a.md", "图谱\n")]);
    let page = search_fulltext_impl(&state, fulltext("   ")).expect("空查询不该报错");
    assert!(page.items.is_empty());
    assert_eq!(page.total, 0);
}

/// 未打开 Vault（没有索引库）时必须返回 E_VAULT_NOT_OPEN，而不是空结果。
#[test]
fn fulltext_without_vault_reports_vault_not_open() {
    let state = AppState::new();
    let err = search_fulltext_impl(&state, fulltext("图谱")).expect_err("未打开 Vault");
    assert_eq!(err.0.code(), "E_VAULT_NOT_OPEN");
}

fn in_file(rel: &str, query: &str, options: FindOptionsArg) -> InFileArgs {
    InFileArgs {
        rel_path: rel.to_string(),
        query: query.to_string(),
        options,
    }
}

fn replace_args(rel: &str, matches: &[InFileMatch], replacement: &str) -> ReplaceArgs {
    ReplaceArgs {
        rel_path: rel.to_string(),
        matches: matches
            .iter()
            .map(|m| ReplaceMatchArg {
                start_byte: m.start_byte,
                end_byte: m.end_byte,
                text: m.text.clone(),
            })
            .collect(),
        replacement: replacement.to_string(),
    }
}

/// FR-SEARCH-40/41：快速打开按**路径片段**模糊匹配，正文里出现不算，附件不进候选。
#[test]
fn quick_open_matches_path_fragments_only() {
    let (_v, state) = fixture(&[
        ("work/2026/report.md", "正文里没有任何关键词\n"),
        ("notes/readme.md", "work/rep 只出现在正文里\n"),
        ("图片.png", "x\n"),
    ]);
    let items = search_quick_open_impl(
        &state,
        QuickOpenArgs {
            query: "work/rep".to_string(),
            limit: None,
        },
    )
    .expect("快速打开");
    assert_eq!(items.len(), 1, "只按文件名/路径匹配（FR-SEARCH-40）");
    assert_eq!(items[0].rel_path, "work/2026/report.md");
    assert_eq!(items[0].name, "report.md");
    assert!(items[0].score > 0);
    assert!(
        items.iter().all(|i| i.rel_path.ends_with(".md")),
        "附件不是笔记"
    );
}

/// 空查询给缺省候选列表（Ctrl+O 未输入时也要有内容），条数受 limit 约束。
#[test]
fn quick_open_empty_query_lists_candidates_with_limit() {
    let (_v, state) = fixture(&[("b.md", "x\n"), ("a.md", "x\n")]);
    let all = search_quick_open_impl(
        &state,
        QuickOpenArgs {
            query: String::new(),
            limit: None,
        },
    )
    .expect("快速打开");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].rel_path, "a.md", "同分时按路径升序（可复现）");
    let one = search_quick_open_impl(
        &state,
        QuickOpenArgs {
            query: String::new(),
            limit: Some(1),
        },
    )
    .expect("快速打开");
    assert_eq!(one.len(), 1);
}

/// FR-SEARCH-30：文件内查找给出「第 X / 共 Y 处」所需的定位与整行文本。
#[test]
fn in_file_search_reports_positions_and_line_text() {
    let (_v, state) = fixture(&[("note.md", "第一行\n见 图谱 与 图谱\n末行\n")]);
    let matches = search_in_file_impl(
        &state,
        in_file("note.md", "图谱", FindOptionsArg::default()),
    )
    .expect("查找");
    assert_eq!(matches.len(), 2, "同一行的两处都要列出");
    assert_eq!(matches[0].line, 2);
    assert_eq!(matches[0].col, 3, "1 基字符列");
    assert_eq!(matches[0].len_chars, 2);
    assert_eq!(matches[0].text, "图谱");
    assert_eq!(matches[0].line_text, "见 图谱 与 图谱");
    assert_eq!(matches[1].col, 8, "第二处按字符列定位");
    let chars: Vec<char> = matches[0].line_text.chars().collect();
    let hit: String = chars[matches[0].highlight_start..matches[0].highlight_end]
        .iter()
        .collect();
    assert_eq!(hit, "图谱", "高亮区间必须正好覆盖命中");
}

/// FR-EDITOR-09 的选项（FR-SEARCH-31 复用）：区分大小写与全字匹配都真的生效。
#[test]
fn in_file_search_options_control_case_and_whole_word() {
    let (_v, state) = fixture(&[("note.md", "Rust rust Rustacean\n")]);
    let loose = search_in_file_impl(
        &state,
        in_file("note.md", "rust", FindOptionsArg::default()),
    )
    .expect("查找");
    assert_eq!(loose.len(), 3, "默认不区分大小写");
    let strict = search_in_file_impl(
        &state,
        in_file(
            "note.md",
            "rust",
            FindOptionsArg {
                case_sensitive: Some(true),
                whole_word: None,
            },
        ),
    )
    .expect("查找");
    assert_eq!(strict.len(), 1, "区分大小写后只剩一处");
    let whole = search_in_file_impl(
        &state,
        in_file(
            "note.md",
            "Rust",
            FindOptionsArg {
                case_sensitive: Some(true),
                whole_word: Some(true),
            },
        ),
    )
    .expect("查找");
    assert_eq!(whole.len(), 1, "全字匹配排除 Rustacean");
}

/// 空查询不返回「全部命中」；文件不存在必须明确失败，而不是静默空数组。
#[test]
fn in_file_search_empty_query_and_missing_file() {
    let (_v, state) = fixture(&[("note.md", "内容\n")]);
    let none = search_in_file_impl(&state, in_file("note.md", "", FindOptionsArg::default()))
        .expect("查找");
    assert!(none.is_empty());
    let err = search_in_file_impl(
        &state,
        in_file("missing.md", "内容", FindOptionsArg::default()),
    )
    .expect_err("文件不存在");
    assert_eq!(err.0.code(), "E_FILE_NOT_FOUND");
}

/// FR-SEARCH-31：全部替换落地（原子写），返回处数与磁盘内容都对得上。
#[test]
fn replace_in_file_applies_all_matches() {
    let (vault, state) = fixture(&[("note.md", "甲 图谱 与 图谱 结尾\n")]);
    let matches = search_in_file_impl(
        &state,
        in_file("note.md", "图谱", FindOptionsArg::default()),
    )
    .expect("查找");
    let result =
        replace_in_file_impl(&state, replace_args("note.md", &matches, "知识图谱")).expect("替换");
    assert_eq!(result.replaced, 2);
    assert_eq!(result.rel_path, "note.md");
    assert!(result.new_mtime_ms > 0, "应返回新 mtime");
    let text = std::fs::read_to_string(vault.path().join("note.md")).expect("读回");
    assert_eq!(text, "甲 知识图谱 与 知识图谱 结尾\n");
}

/// 陈旧检测（P-2 / R-07）：查找之后文件被外部改过 → 整体中止且**磁盘零改动**。
#[test]
fn replace_in_file_detects_stale_targets_and_writes_nothing() {
    let (vault, state) = fixture(&[("note.md", "图谱 在这里\n")]);
    let matches = search_in_file_impl(
        &state,
        in_file("note.md", "图谱", FindOptionsArg::default()),
    )
    .expect("查找");
    std::fs::write(vault.path().join("note.md"), "别的 在这里\n").expect("外部改写");
    let err = replace_in_file_impl(&state, replace_args("note.md", &matches, "知识图谱"))
        .expect_err("陈旧目标必须拒绝");
    assert_eq!(err.0.code(), "E_WRITE_CONFLICT");
    let text = std::fs::read_to_string(vault.path().join("note.md")).expect("读回");
    assert_eq!(text, "别的 在这里\n", "拒绝时不得留下任何写入痕迹");
}

/// 受保护目录（.knowlpad 等）不得被改写（FR-STORAGE-02/04；DEBT-16 的护栏）。
#[test]
fn replace_in_file_rejects_protected_directory() {
    let (_v, state) = fixture(&[("note.md", "图谱\n")]);
    let err = replace_in_file_impl(
        &state,
        ReplaceArgs {
            rel_path: ".knowlpad/index.db".to_string(),
            matches: vec![ReplaceMatchArg {
                start_byte: 0,
                end_byte: 1,
                text: "x".to_string(),
            }],
            replacement: "y".to_string(),
        },
    )
    .expect_err("受保护目录必须拒绝");
    assert_eq!(err.0.code(), "E_PATH_ESCAPE_DENY");
}

/// 没有目标时是**空操作**（不是错误），内容原样保留。
#[test]
fn replace_in_file_without_matches_is_noop() {
    let (vault, state) = fixture(&[("note.md", "原文\n")]);
    let path = vault.path().join("note.md");
    let before = kp_domain::note_io::mtime_ms(&path).expect("读 mtime");
    let result = replace_in_file_impl(&state, replace_args("note.md", &[], "新")).expect("空目标");
    assert_eq!(result.replaced, 0);
    assert_eq!(
        std::fs::read_to_string(&path).expect("读回"),
        "原文\n",
        "内容不得变化"
    );
    assert_eq!(
        kp_domain::note_io::mtime_ms(&path).expect("读 mtime"),
        before,
        "空目标不该写盘：写一次内容相同的文件会刷新 mtime，编辑器会误判为外部修改"
    );
}

/// FR-SEARCH-10：历史**只在显式要求时**记录（options.recordHistory），最新在前，可清空且幂等。
#[test]
fn history_records_only_on_request_and_can_be_cleared() {
    let (_v, state) = fixture(&[("a.md", "图谱\n")]);
    search_fulltext_impl(&state, fulltext("图谱")).expect("搜索");
    assert!(
        search_history_get_impl(&state).expect("读历史").is_empty(),
        "缺省不记录：只读命令不得有副作用"
    );

    let mut args = fulltext("图谱");
    args.options.record_history = Some(true);
    search_fulltext_impl(&state, args).expect("搜索");
    let mut second = fulltext("力导向");
    second.options.record_history = Some(true);
    search_fulltext_impl(&state, second).expect("搜索");
    assert_eq!(
        search_history_get_impl(&state).expect("读历史"),
        vec!["力导向".to_string(), "图谱".to_string()],
        "最新在前"
    );

    search_history_clear_impl(&state).expect("清空");
    assert!(search_history_get_impl(&state).expect("读历史").is_empty());
    search_history_clear_impl(&state).expect("清空是幂等的");
}

/// 全局库不可用时读历史必须**明确报错**（来源缺失 ≠ 空历史）。
#[test]
fn history_without_global_db_reports_error() {
    let state = AppState::new();
    let err = search_history_get_impl(&state).expect_err("没有全局库");
    assert_eq!(err.0.code(), "E_DB_ERROR");
}

// ── 高级语法（FR-SEARCH-20~27）与语法错误提示（AC-SEARCH-05）────────────────

/// 文件较多时按 (相对路径, 正文) 建夹具（AC-SEARCH-02 需要 50 篇的规模）。
fn fixture_many(files: &[(String, String)]) -> (tempfile::TempDir, AppState) {
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    fixture(&refs)
}

/// AC-SEARCH-02 的原文场景：50 篇带 `#工作/2026`，其中 8 篇含短语「季度报告」、
/// 2 篇同时含「草稿」⇒ `tag:#工作/2026 "季度报告" -草稿` 必须返回 **6** 条（8 − 2）。
#[test]
fn ac_search_02_advanced_syntax_combination_returns_six() {
    let mut files: Vec<(String, String)> = Vec::new();
    for i in 0..50 {
        let body = match i {
            // 前 2 篇：含短语 + 草稿（应被 -草稿 排除）
            0..=1 => format!("#工作/2026 季度报告 草稿 编号{i}\n"),
            // 接着 6 篇：含短语（应命中）
            2..=7 => format!("#工作/2026 季度报告 编号{i}\n"),
            _ => format!("#工作/2026 其它内容 编号{i}\n"),
        };
        files.push((format!("work/2026/n{i:02}.md"), body));
    }
    let (_v, state) = fixture_many(&files);
    let page =
        search_fulltext_impl(&state, fulltext("tag:#工作/2026 \"季度报告\" -草稿")).expect("搜索");
    assert_eq!(
        page.total, 6,
        "8 篇含短语、其中 2 篇含草稿 → 6（AC-SEARCH-02 的原文场景）"
    );
    assert_eq!(page.items.len(), 6, "本页条数与 total 一致");
    for item in &page.items {
        assert!(item.hit_count >= 1, "每条都应命中短语：{item:?}");
    }
}

/// FR-SEARCH-23：`line:(...)` 要求组内词在**同一行**共现 —— 分处两行的笔记不命中。
#[test]
fn line_filter_requires_terms_on_one_line() {
    let (_v, state) = fixture(&[
        ("same.md", "季度 报告 在同一行\n别的行\n"),
        ("split.md", "季度 在第一行\n另一行 报告\n"),
        ("none.md", "毫不相关\n"),
    ]);
    let page = search_fulltext_impl(&state, fulltext("line:(季度 报告)")).expect("搜索");
    let paths: Vec<&str> = page.items.iter().map(|i| i.rel_path.as_str()).collect();
    assert_eq!(page.total, 1, "只有同行共现的那篇：{paths:?}");
    assert_eq!(paths, vec!["same.md"]);
}

/// FR-SEARCH-22：只写 `tag:#工作` 也要能列笔记（纯过滤查询，没有全文词）。
#[test]
fn tag_only_query_lists_tagged_notes() {
    let (_v, state) = fixture(&[("a.md", "#工作/2026 甲\n"), ("b.md", "#私事 乙\n")]);
    let page = search_fulltext_impl(&state, fulltext("tag:#工作")).expect("搜索");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].rel_path, "a.md");
    assert!(!page.has_more);
}

/// FR-SEARCH-20/21：path: 与 file: 是**过滤条件**，正文里出现同样的字不算命中。
#[test]
fn path_and_file_filters_do_not_search_the_body() {
    let (_v, state) = fixture(&[
        ("work/report.md", "季度 内容\n"),
        ("other/report.md", "季度 内容\n"),
        (
            "other/note.md",
            "work/report.md 只是正文里的一串字，也含 内容 二字\n",
        ),
    ]);
    let by_path = search_fulltext_impl(&state, fulltext("path:work/ 季度")).expect("搜索");
    assert_eq!(by_path.total, 1, "只有 work/ 下的那篇");
    assert_eq!(by_path.items[0].rel_path, "work/report.md");
    let by_file = search_fulltext_impl(&state, fulltext("file:note 内容")).expect("搜索");
    assert_eq!(by_file.total, 1, "只有文件名带 note 的那篇");
    assert_eq!(by_file.items[0].rel_path, "other/note.md");
}

/// AC-SEARCH-05：未闭合引号必须给出**明确的语法错误提示**，
/// 而不是静默返回空结果（本夹具的正文里**确实**有这串字，静默空结果会被这条用例抓住）。
#[test]
fn ac_search_05_unclosed_quote_reports_syntax_error_not_empty_results() {
    let (_v, state) = fixture(&[("a.md", "未闭合的短语 就在正文里\n")]);
    let err = search_fulltext_impl(&state, fulltext("\"未闭合的短语")).expect_err("必须报语法错误");
    assert_eq!(err.0.code(), "E_SEARCH_SYNTAX");
    assert_eq!(err.0.kind(), "SearchSyntax");
    let message = err.0.to_string();
    assert!(
        message.contains("双引号未闭合"),
        "提示必须指出原因：{message}"
    );
    assert!(
        !message.contains("未闭合的短语"),
        "提示不得回显用户查询串（SEC-09 的同一纪律）：{message}"
    );
}

/// FR-SEARCH-27：`line:(` 括号未闭合同样是语法错误，不得静默当成普通词。
#[test]
fn unclosed_line_group_reports_syntax_error() {
    let (_v, state) = fixture(&[("a.md", "任意内容\n")]);
    let err =
        search_fulltext_impl(&state, fulltext("line:(季度 报告")).expect_err("必须报语法错误");
    assert_eq!(err.0.code(), "E_SEARCH_SYNTAX");
    assert!(err.0.to_string().contains("括号未闭合"), "{err}");
}

/// 只有键没有值的过滤条件要明说，不能静默忽略（否则用户以为过滤生效了）。
#[test]
fn empty_filter_value_reports_syntax_error() {
    let (_v, state) = fixture(&[("a.md", "任意内容\n")]);
    let err = search_fulltext_impl(&state, fulltext("tag: 内容")).expect_err("必须报语法错误");
    assert_eq!(err.0.code(), "E_SEARCH_SYNTAX");
}

// ── 2026-10-15 三项裁定的回归（口径见 PRD §5.3.4.3）──────────────────────────

/// **裁定①** + FR-SEARCH-25：`-草稿` 单独使用时返回**补集**（全集 − 含草稿的笔记），
/// 而不是「含草稿的笔记」——M3 的旧语义在本刀被改正。
#[test]
fn exclude_only_query_returns_the_complement_not_the_hits() {
    let (_v, state) = fixture(&[
        ("draft.md", "草稿 在这里\n"),
        ("a.md", "正常内容\n"),
        ("b.md", "另一篇正常内容\n"),
    ]);
    let page = search_fulltext_impl(&state, fulltext("-草稿")).expect("搜索");
    let paths: Vec<&str> = page.items.iter().map(|i| i.rel_path.as_str()).collect();
    assert_eq!(page.total, 2, "3 篇 − 含草稿的 1 篇 = 2：{paths:?}");
    assert!(
        !paths.contains(&"draft.md"),
        "含排除词的笔记不得出现在结果里：{paths:?}"
    );
    // 对照：正向查询仍命中那一篇（证明上面不是「什么都没搜到」）
    let hit = search_fulltext_impl(&state, fulltext("草稿")).expect("搜索");
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items[0].rel_path, "draft.md");
}

/// **裁定③**（PRD §5.3.4.3）：`line:` 候选数超过上限时必须给出「请先缩小范围」的**明确提示**，
/// 而不是静默空结果或长时间阻塞。上限由参数注入，故不必造 5001 篇笔记。
#[test]
fn line_filter_reports_a_hint_when_candidates_exceed_the_cap() {
    let (_v, state) = fixture(&[
        ("same.md", "季度 报告 在同一行\n"),
        ("split.md", "季度 在第一行\n另一行 报告\n"),
    ]);
    let pool = state.index_db().expect("索引库");
    let root = crate::commands::paths::root_of(&state).expect("Vault 根");
    let parsed =
        kp_domain::search::parse_query("line:(季度 报告)", kp_domain::search::MatchMode::All)
            .expect("解析");
    let view = ViewOptions {
        context_lines: 2,
        max_snippets: 5,
    };
    let window = PageWindow {
        offset: 0,
        limit: 20,
    };
    // 两个候选、上限 1 ⇒ 必须报错并带提示文案
    let err = page_with_line_filter(&pool, &root, &parsed, SortOrder::Path, view, window, 1)
        .expect_err("超限必须报错");
    assert_eq!(err.0.code(), "E_SEARCH_SYNTAX", "与语法错误共用提示通道");
    assert!(err.0.to_string().contains("缩小范围"), "{err}");
    // 边界：上限**等于**候选数时放行（是「超过」才拒绝）
    let ok = page_with_line_filter(&pool, &root, &parsed, SortOrder::Path, view, window, 2)
        .expect("等于上限应放行");
    assert_eq!(ok.total, 1, "只有同行共现的那篇命中");
    assert_eq!(ok.items.len(), 1);
    assert_eq!(ok.items[0].rel_path, "same.md");
}

/// **裁定③** 的纯函数边界；顺带钉住上限常量的取值 ——
/// 改它必须同步 PRD §5.3.4.3 与 perf/d25-budgets.json。
///
/// 300 是 **M5 批次 5 的实测标定值**（最坏情况 ≈3.2 ms/候选；300 条实测 1070–1073 ms），
/// 不是估计值。这条断言是**有意设置的绊线**：改常量就会红，逼着改的人同时更新真源与预算表。
#[test]
fn candidate_guard_boundary_and_documented_cap() {
    assert_eq!(
        MAX_LINE_FILTER_CANDIDATES, 300,
        "上限改动必须同步 PRD §5.3.4.3 与 perf/d25-budgets.json"
    );
    assert!(guard_candidate_count(300, 300).is_ok(), "等于上限放行");
    let err = guard_candidate_count(301, 300).expect_err("超过上限必须报错");
    assert_eq!(err.0.code(), "E_SEARCH_SYNTAX");
    assert!(err.0.to_string().contains("缩小范围"), "{err}");
}
