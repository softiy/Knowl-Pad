//! kp-domain 的构建脚本：计算**分词词典指纹**（DEBT-28 / 技术方案 §4.6）。
//!
//! 为什么需要它：`tokenizer_dict_hash` 进索引签名，签名变了既有索引才判废重建；
//! 之前它是**手写常量**（FNV-1a64），jieba-rs 升级后不会自动变 ⇒ 可能复用过期分词索引。
//!
//! 口径（与 `src/tokenizer_meta.rs` 的模块头一致）：词典在 jieba-rs 内是**私有 static**（还被 zstd 压过），
//! 仓库里没有词典文件 ⇒ 没有可哈希的词典字节；因此对"能导致词典变化的那条因果链"算 SHA-256 前 16 位：
//! **解析出的 jieba-rs 版本 + feature 集合（含 default-features）+ TOKENIZER_VERSION + 包版本 + 自定义词典**。
//! 版本从 `Cargo.lock` 读，并 `rerun-if-changed` 盯住它 —— 升级 jieba-rs 会自动改指纹。
//!
//! **自定义词典钩子**：若将来有 `assets/tokenizer_dict.txt`，它会被读入并计入清单（`custom-dict=sha256:<16>`），
//! 同时 `rerun-if-changed` 盯住该文件；目前没有这个文件，所以清单是**退化**的（`custom-dict=none`）。
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/tokenizer_meta.rs"]
mod tokenizer_meta;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    println!("cargo:rerun-if-changed=src/tokenizer_meta.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let lock = manifest_dir.join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());

    let jieba_version = jieba_version_from_lock(&lock)
        .unwrap_or_else(|err| panic!("无法从 Cargo.lock 解析 jieba-rs 版本：{err}"));
    let (features, default_enabled) =
        jieba_features_from_manifest(&manifest_dir.join("Cargo.toml"));
    let features_text = features_text(&features, default_enabled);
    let custom = custom_dict_digest(&manifest_dir);
    let text = tokenizer_meta::manifest(
        &jieba_version,
        &features_text,
        tokenizer_meta::TOKENIZER_VERSION,
        env!("CARGO_PKG_VERSION"),
        &custom,
    );
    let hash = tokenizer_meta::hash_prefix(text.as_bytes());
    println!("cargo:rustc-env=KP_TOKENIZER_DICT_HASH={hash}");
    println!("cargo:rustc-env=KP_TOKENIZER_DICT_MANIFEST={text}");
}

/// 去掉 Cargo 文本值两端的引号。
fn unquote(raw: &str) -> String {
    raw.trim().trim_matches('"').to_string()
}

/// 从 `Cargo.lock` 里取出**解析后**的 jieba-rs 版本（`[[package]]` 块内的 `version`）。
fn jieba_version_from_lock(path: &Path) -> Result<String, String> {
    let text =
        fs::read_to_string(path).map_err(|err| format!("读取 {} 失败：{err}", path.display()))?;
    let mut in_jieba = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            in_jieba = false;
            continue;
        }
        if in_jieba {
            if let Some(rest) = trimmed.strip_prefix("version = ") {
                return Ok(unquote(rest));
            }
        }
        if trimmed.starts_with("name = ") && unquote(&trimmed["name = ".len()..]) == "jieba-rs" {
            in_jieba = true;
        }
    }
    Err("Cargo.lock 里没有 jieba-rs 条目".to_string())
}

/// 本仓对 jieba-rs 的声明片段（`jieba-rs = …` 行或多行内联表，或 `[dependencies.jieba-rs]` 表）。
fn jieba_decl_fragment(manifest: &str) -> String {
    let mut out = String::new();
    let mut collecting = false;
    let mut depth: i32 = 0;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if !collecting {
            let starts_inline = trimmed.starts_with("jieba-rs") && trimmed.contains('=');
            let starts_table = trimmed == "[dependencies.jieba-rs]";
            if starts_inline || starts_table {
                collecting = true;
                depth += trimmed.matches('{').count() as i32 - trimmed.matches('}').count() as i32;
                out.push_str(trimmed);
                out.push('\n');
                if starts_table || depth <= 0 {
                    if starts_table {
                        continue; // 表形式：继续收，直到下一个 section
                    }
                    break;
                }
            }
            continue;
        }
        if trimmed.starts_with('[') {
            break; // 表形式结束
        }
        depth += trimmed.matches('{').count() as i32 - trimmed.matches('}').count() as i32;
        out.push_str(trimmed);
        out.push('\n');
        if depth <= 0 {
            break;
        }
    }
    out
}

/// 解析 feature 列表与 `default-features`（缺省为 true）。
fn jieba_features_from_manifest(path: &Path) -> (Vec<String>, bool) {
    let manifest =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("读取 Cargo.toml 失败：{err}"));
    let fragment = jieba_decl_fragment(&manifest);
    let mut features: Vec<String> = Vec::new();
    if let Some(pos) = fragment.find("features") {
        let rest = &fragment[pos..];
        if let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) {
            if open < close {
                for item in rest[open + 1..close].split(',') {
                    let name = unquote(item);
                    if !name.is_empty() {
                        features.push(name);
                    }
                }
            }
        }
    }
    features.sort();
    features.dedup();
    let default_enabled = !fragment
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .contains("default-features = false");
    (features, default_enabled)
}

/// 清单里的 feature 字段：排序后的列表（无则 `-`）+ `default` 标志（翻 feature 也必须改指纹）。
fn features_text(features: &[String], default_enabled: bool) -> String {
    let list = if features.is_empty() {
        "-".to_string()
    } else {
        features.join(",")
    };
    format!("{list};default={default_enabled}")
}

/// 自定义词典钩子：有文件就把它算进清单并盯住；没有则 `none`。
fn custom_dict_digest(manifest_dir: &Path) -> String {
    let path = manifest_dir.join("assets/tokenizer_dict.txt");
    if !path.exists() {
        return "none".to_string();
    }
    println!("cargo:rerun-if-changed=assets/tokenizer_dict.txt");
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("读取自定义词典失败：{err}"));
    println!("cargo:warning=检测到自定义分词词典，已并入指纹（assets/tokenizer_dict.txt）");
    format!("sha256:{}", tokenizer_meta::hash_prefix(&bytes))
}
