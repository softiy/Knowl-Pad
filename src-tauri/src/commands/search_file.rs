//! 文件内查找与替换的命令体（PRD §5.3.4 的 search_in_file / replace_in_file）。
//!
//! 查找选项复用 FR-EDITOR-09（FR-SEARCH-31 明示「复用 FR-EDITOR-09」）；纯逻辑在
//! kp_domain::find_in_file。替换是 ⚠️ 写操作，安全链有三段，缺一不可：
//! ① **受保护目录**不得改写（FR-STORAGE-02/04，DEBT-16）；
//! ② **逐条比对期望原文**——查找之后文件被外部改过时，按旧偏移硬改就是静默破坏用户数据（P-2 / R-07）；
//! ③ **原子写 + base_mtime**（R-06 / NFR-REL-01 / FR-EDITOR-34）：mtime 取**读正文之前**的值，
//!    因此读取期间的任何外部改动都会被检出并整体中止。

use std::path::Path;

use crate::commands::paths::{resolve_in, root_of};
use crate::commands::search::{FindOptionsArg, InFileArgs, ReplaceArgs, ReplaceMatchArg};
use crate::commands::search_view::{InFileMatch, ReplaceResult};
use crate::error_wrapper::KpError;
use crate::state::AppState;
use kp_domain::error::AppError;
use kp_domain::find_in_file::{
    apply_replacements, find_all_with_lines, FindOptions, LocatedMatch, ReplaceTarget,
};
use kp_domain::note_io;

/// UTF-8 解码（ERR-02：不把编码库的英文原文透给用户，细节进日志）。
fn decode_text(bytes: Vec<u8>) -> Result<String, KpError> {
    String::from_utf8(bytes).map_err(|err| {
        tracing::warn!(error = %err, "文件不是有效 UTF-8");
        KpError(AppError::IoFailure(
            "该文件不是有效的 UTF-8 文本，可能不是 Markdown 笔记或已损坏；请用外部编辑器确认编码"
                .to_string(),
        ))
    })
}

/// 读一篇笔记的正文（SEC-02 七步校验；文件不存在是**明确失败**，不是静默空结果）。
fn read_text(root: &Path, rel_path: &str) -> Result<String, KpError> {
    let path = resolve_in(root, rel_path)?;
    decode_text(note_io::read_note(&path).map_err(KpError)?)
}

/// 选项映射（缺省：不区分大小写、非全字匹配 —— 与编辑器的常见默认一致）。
fn find_options(arg: &FindOptionsArg) -> FindOptions {
    FindOptions {
        case_sensitive: arg.case_sensitive.unwrap_or(false),
        whole_word: arg.whole_word.unwrap_or(false),
    }
}

/// IPC 目标 → 域层目标。
fn to_targets(matches: &[ReplaceMatchArg]) -> Vec<ReplaceTarget> {
    matches
        .iter()
        .map(|m| ReplaceTarget {
            start_byte: m.start_byte,
            end_byte: m.end_byte,
            expect: m.text.clone(),
        })
        .collect()
}

/// 文件内查找（FR-SEARCH-30：「第 X / 共 Y 处」= 数组下标与长度）。
pub(crate) fn search_in_file_impl(
    state: &AppState,
    args: InFileArgs,
) -> Result<Vec<InFileMatch>, KpError> {
    let root = root_of(state)?;
    let content = read_text(&root, &args.rel_path)?;
    let located: Vec<LocatedMatch> =
        find_all_with_lines(&content, &args.query, find_options(&args.options));
    Ok(located
        .iter()
        .map(|m| InFileMatch::of(&content, m))
        .collect())
}

/// 文件内替换（⚠️ 写操作；FR-SEARCH-31）。
pub(crate) fn replace_in_file_impl(
    state: &AppState,
    args: ReplaceArgs,
) -> Result<ReplaceResult, KpError> {
    // ① 受保护目录（.knowlpad / .obsidian / .git…）不得被改写（FR-STORAGE-02/04）
    kp_domain::file_ops::ensure_writable(&args.rel_path).map_err(KpError)?;
    let root = root_of(state)?;
    let path = resolve_in(&root, &args.rel_path)?;
    // ③ 的基准值必须**先于**读正文取得：这样读取期间的任何外部改动都会被 write_note 检出
    let base_mtime = note_io::mtime_ms(&path).map_err(KpError)?;
    let content = decode_text(note_io::read_note(&path).map_err(KpError)?)?;
    let targets = to_targets(&args.matches);
    if targets.is_empty() {
        // 没有目标 = 无改动：**不写盘**。写一次「内容相同」的文件会刷新 mtime，
        // 而编辑器正拿着旧的 baseMtime 做冲突检测（FR-EDITOR-34）—— 那会平白弹出一次「文件已被外部修改」。
        return Ok(ReplaceResult {
            rel_path: args.rel_path,
            replaced: 0,
            new_mtime_ms: base_mtime,
        });
    }
    // ② 逐条校验期望原文；任一处对不上即整体中止（返回 E_WRITE_CONFLICT，磁盘零改动）
    let updated = apply_replacements(&content, &targets, &args.replacement).map_err(KpError)?;
    let replaced = targets.len();
    let new_mtime =
        note_io::write_note(&path, updated.as_bytes(), Some(base_mtime)).map_err(KpError)?;
    // CMD-06 / ERR-04：⚠️ 写操作留痕。**不含正文与搜索词**（SEC-09 / R-12）。
    // 索引由文件监听在 2s 窗口内增量刷新（AC-SEARCH-04），此处不主动重建索引。
    tracing::info!(
        rel_path = %args.rel_path,
        replaced,
        "文件内替换完成（原子写入）"
    );
    Ok(ReplaceResult {
        rel_path: args.rel_path,
        replaced,
        new_mtime_ms: new_mtime,
    })
}
