//! 词典指纹口径的用例（DEBT-28）：**变更感知**本身必须被钉住，而不是只测一次值。

use super::*;

/// 已知向量：SHA-256("abc") 的前 16 位十六进制（外部可复核，防"自家算自家"）。
#[test]
fn hash_prefix_matches_known_sha256_vector() {
    assert_eq!(hash_prefix(b"abc"), "ba7816bf8f01cfea");
}

#[test]
fn hash_prefix_is_16_lowercase_hex() {
    let out = hash_prefix(b"anything");
    assert_eq!(out.len(), DICT_HASH_LEN);
    assert!(
        out.chars()
            .all(|c| c.is_ascii_digit() || (c.is_ascii_lowercase() && c.is_ascii_hexdigit())),
        "必须是十六进制小写：{out}"
    );
}

#[test]
fn identical_bytes_yield_identical_prefix() {
    assert_eq!(
        hash_prefix(b"jieba dict bytes"),
        hash_prefix(b"jieba dict bytes")
    );
}

#[test]
fn different_bytes_yield_different_prefix() {
    assert_ne!(
        hash_prefix(b"jieba dict bytes A"),
        hash_prefix(b"jieba dict bytes B")
    );
}

/// 库里的常量必须等于"对同一份清单重算"的结果 —— 防止 build.rs 与库两侧口径漂移。
#[test]
fn build_time_hash_matches_manifest_recomputation() {
    let recomputed = hash_prefix(crate::tokenize::dict_manifest().as_bytes());
    assert_eq!(crate::tokenize::dict_hash(), recomputed);
}

/// 指纹确实是 16 位小写十六进制（进签名前的基本形状）。
#[test]
fn build_time_hash_shape() {
    let hash = crate::tokenize::dict_hash();
    let text = crate::tokenize::dict_manifest();
    // 打印实际值（测试输出即证据：指纹与它对应的清单必须一起可读）
    println!("KP_TOKENIZER_DICT_HASH = {hash}");
    println!("KP_TOKENIZER_DICT_MANIFEST = {text}");
    assert_eq!(hash.len(), DICT_HASH_LEN);
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{hash}"
    );
}

/// **变更感知**：清单里任一输入变化，指纹必须变（这是整套机制的目的）。
#[test]
fn any_manifest_input_change_changes_the_hash() {
    let base = manifest("0.11.0", "-", TOKENIZER_VERSION, "0.1.0", "none");
    let base_hash = hash_prefix(base.as_bytes());
    let variants = [
        manifest("0.12.0", "-", TOKENIZER_VERSION, "0.1.0", "none"),
        manifest("0.11.0", "simd", TOKENIZER_VERSION, "0.1.0", "none"),
        manifest("0.11.0", "-", "0.11.0-next", "0.1.0", "none"),
        manifest("0.11.0", "-", TOKENIZER_VERSION, "0.2.0", "none"),
        manifest(
            "0.11.0",
            "-",
            TOKENIZER_VERSION,
            "0.1.0",
            "sha256:deadbeefdeadbeef",
        ),
    ];
    for variant in variants {
        let hash = hash_prefix(variant.as_bytes());
        assert_ne!(hash, base_hash, "输入变化必须改变指纹：{variant}");
    }
    // 相同输入必须得到相同指纹（确定性）
    assert_eq!(hash_prefix(base.as_bytes()), base_hash);
}

/// 清单本身必须带口径标签与全部字段（少一个字段 = 少一条变更感知路径）。
#[test]
fn manifest_carries_every_field_and_tag() {
    let text = manifest("0.11.0", "a,b", "0.11.0", "0.1.0", "none");
    for part in [
        DICT_MANIFEST_TAG,
        "jieba-rs=0.11.0",
        "features=a,b",
        "tokenizer=0.11.0",
        "package=0.1.0",
        "custom-dict=none",
    ] {
        assert!(text.contains(part), "清单缺少 {part}：{text}");
    }
}

/// 真实清单里必须带上当前解析出的 jieba-rs 版本（不能是 `-`/空），否则等于没盯住版本。
#[test]
fn real_manifest_records_a_jieba_version() {
    let text = crate::tokenize::dict_manifest();
    let version = text
        .split(';')
        .find_map(|part| part.strip_prefix("jieba-rs="))
        .expect("清单必须含 jieba-rs 字段");
    assert!(
        !version.is_empty() && version != "-",
        "版本不得为空：{text}"
    );
    assert!(
        version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "版本应以数字开头：{version}"
    );
}
