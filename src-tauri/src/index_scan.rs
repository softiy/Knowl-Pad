//! 索引的**扫描侧**：目录遍历、文件元信息、命名与内容指纹。
//!
//! 从 `index_engine.rs` 拆出（CODE-11 的硬上限是 350 SLOC）：这里回答"盘上有什么"，
//! 引擎那边回答"怎么把它写进库"。**对外路径不变** —— `index_engine` 用 `pub use` 重新导出，
//! 既有调用方（`index_watch_paths` / `index_query` / 各测试）一行都不用改。
//!
//! 忽略规则**只此一份**：文件树与监听侧都复用它，避免出现两套真相。

use std::path::Path;

use walkdir::WalkDir;

use kp_domain::error::AppError;

/// 遍历时永远跳过的目录（与监听侧共用；文件树也复用它保持同一套真相）。
pub const IGNORED_DIRS: [&str; 4] = [".knowlpad", ".obsidian", ".git", "node_modules"];
/// 超过此大小的笔记跳过（SEC-11）。
pub const MAX_NOTE_BYTES: u64 = 5 * 1024 * 1024;

/// 待索引的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    pub rel_path: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
}

pub(crate) fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn scan_vault(root: &Path) -> Result<(Vec<ScanEntry>, Vec<String>), AppError> {
    let profile_token = crate::index_profile::start();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if name.starts_with(".kp-tmp-") {
            return false;
        }
        !(e.file_type().is_dir() && IGNORED_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d)))
    });
    for item in walker {
        let entry = match item {
            Ok(e) => e,
            Err(err) => {
                warnings.push(format!("遍历告警:{err}"));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // PR-6：记录**全部**文件（附件也要出现在文件树里）；只有 .md 会被解析并建全文索引
        let is_note = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"));
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let rel_path = path
            .strip_prefix(root)
            .map_err(|_| AppError::PathOutsideVault)?
            .to_string_lossy()
            .replace('\\', "/");
        if is_note && size_bytes > MAX_NOTE_BYTES {
            warnings.push(format!("skip-oversize:{rel_path}"));
            continue;
        }
        entries.push(ScanEntry {
            rel_path,
            size_bytes,
            mtime_ms: mtime_ms(path),
        });
    }
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    crate::index_profile::stop(profile_token, crate::index_profile::Phase::Scan);
    Ok((entries, warnings))
}

/// 该目录名是否在索引忽略集里（文件树复用，避免两套真相）。
pub fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
}

/// 简易内容指纹（FNV-1a 64）：仅供增量索引判断「内容是否变化」，不承担安全用途。
pub fn content_fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn kind_of(rel_path: &str) -> &'static str {
    let ext = rel_path.rsplit('.').next().unwrap_or("").to_lowercase();
    if ext == "md" {
        return "note";
    }
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "pdf" | "mp3" | "wav" | "m4a"
        | "mp4" | "mov" | "zip" | "txt" | "csv" | "json" => "attachment",
        _ => "other",
    }
}

pub(crate) fn split_name(rel_path: &str) -> (String, String, String) {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path).to_string();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => (name.clone(), String::new()),
    };
    (name, stem, ext)
}
