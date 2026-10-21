//! 分词词典指纹的**构建期口径**（DEBT-28；技术方案 §4.6）。
//!
//! **为什么是对"清单"算哈希，而不是对词典字节算哈希**：
//! jieba-rs 的词典是**编译期内嵌**的（`lib.rs` 里 `include_flate!` 进一个私有 `static`，还被 zstd 压过），
//! 仓库里没有独立词典文件、crate 也没有公开访问器 ⇒ **没有可哈希的词典字节**。而词典唯一能变的途径就是
//! **升级 jieba-rs**（crates.io 不允许同版本重发），所以指纹算在"能导致词典变化的那条因果链"上：
//! 解析出的 jieba-rs 版本 + 它在本仓的 feature 集合 + `TOKENIZER_VERSION` + kp-domain 包版本 + 自定义词典（若有）。
//!
//! **现在的清单是"退化"的**：`custom-dict=none`，因为我们还没有自定义词典。**将来若引入 jieba 的 `with_dict`**
//! （自定义词典文件），必须把**该文件的字节**并入清单（`build.rs` 里已预留钩子：存在 `assets/tokenizer_dict.txt`
//! 时会把它算进去并 `rerun-if-changed`）—— 那时才是"对词典内容算哈希"。
//!
//! 本文件同时被 `build.rs`（`#[path]` 引入）与库使用：**单一实现**，避免两处口径漂移。
use sha2::{Digest, Sha256};

/// 清单口径标签（改口径必须换标签 —— 否则旧值会被误认为新口径的结果）。
pub const DICT_MANIFEST_TAG: &str = "kp-tokenizer-dict-v1";

/// 分词器版本（进索引签名；**升级 jieba-rs 时必须同步修改**，否则签名不变、不会触发重建）。
pub const TOKENIZER_VERSION: &str = "0.11.0";

/// 指纹长度（十六进制字符数；技术方案 §4.6 约定"前 16 位"）。
pub const DICT_HASH_LEN: usize = 16;

/// SHA-256 的十六进制小写，截取前 [`DICT_HASH_LEN`] 位。
///
/// 说明：sha2 0.11 的 `finalize()` 不再实现 `LowerHex`，因此手动按字节格式化。
pub fn hash_prefix(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let full: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    full[..DICT_HASH_LEN].to_string()
}

/// 规范清单（固定顺序、`;` 分隔、无换行）。
///
/// **每一个参数都是"变更感知"的输入**：任一字段变化都必须改变指纹 —— 由 `tokenizer_meta_tests` 钉住。
///
/// - `jieba_version`：**解析后**的版本（来自 `Cargo.lock`，不是 manifest 里的版本区间）；
/// - `jieba_features`：本仓对 jieba-rs 启用的 feature 集合（排序、逗号连接；无则 `-`）；
/// - `package_version`：kp-domain 的包版本；
/// - `custom_dict`：自定义词典内容的哈希（无则 `none`；见模块头）。
pub fn manifest(
    jieba_version: &str,
    jieba_features: &str,
    tokenizer_version: &str,
    package_version: &str,
    custom_dict: &str,
) -> String {
    format!(
        "{DICT_MANIFEST_TAG};jieba-rs={jieba_version};features={jieba_features};tokenizer={tokenizer_version};package={package_version};custom-dict={custom_dict}"
    )
}

#[cfg(test)]
#[path = "tokenizer_meta_tests.rs"]
mod tests;
