#!/usr/bin/env node
// 校验：`kp-domain` 等 workspace 成员对 **jieba-rs** 的「声明 feature 集合」是否等于「cargo 解析后实际激活的集合」。
//
// 为什么需要（DEBT-28 遗留）：词典指纹的清单里写了 `features=…`，而 build.rs 是**自己读 Cargo.toml** 得到这串的；
// 若 Cargo 的实际解析与声明不一致（例如 workspace 里另一个成员又开了别的 feature、或 `default-features` 被继承改写），
// 指纹就会「声称覆盖了一个并不存在（或漏掉一个实际存在）的 feature 集合」。这条门禁把两边对齐。
//
// 口径：
//   声明侧 = workspace 内所有依赖 `jieba-rs` 的成员所声明的 features 并集 + 是否用 default
//   期望侧 = 声明侧按 jieba-rs **自己的 features 定义**求闭包（`dep:` / `pkg/feat` 项跳过，只跟本地 feature 名）
//   实际侧 = `cargo metadata` 的 `resolve.nodes[].features`（cargo 真正激活的集合）
// 用法：node scripts/check-jieba-features.mjs [--self-test]
import { execFileSync } from "node:child_process";
import process from "node:process";

const selfTest = process.argv.includes("--self-test");

function fail(message) {
  console.error("❌ " + message);
  process.exit(1);
}

const raw = execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
});
const meta = JSON.parse(raw);
const members = new Set(meta.workspace_members);
const jieba = meta.packages.find((p) => p.name === "jieba-rs");
if (!jieba) fail("cargo metadata 里没有 jieba-rs");
const node = meta.resolve.nodes.find((n) => n.id === jieba.id);
if (!node) fail("resolve.nodes 里没有 jieba-rs（--no-deps 或离线解析失败？）");

const declared = new Set();
let usesDefault = false;
for (const pkg of meta.packages.filter((p) => members.has(p.id))) {
  const dep = pkg.dependencies.find((d) => d.name === "jieba-rs");
  if (!dep) continue;
  for (const f of dep.features ?? []) declared.add(f);
  if (dep.uses_default_features) usesDefault = true;
}

// 按 jieba-rs 自己的 features 定义求闭包（default → default-dict 等）
function closure(seeds, withDefault) {
  const queue = [...seeds];
  if (withDefault) queue.push("default");
  const seen = new Set();
  while (queue.length > 0) {
    const feature = queue.pop();
    if (seen.has(feature)) continue;
    seen.add(feature);
    for (const entry of jieba.features[feature] ?? []) {
      // `dep:xxx` 与 `pkg/feat` 不是本地 feature，不参与本集合比较
      if (entry.startsWith("dep:") || entry.includes("/")) continue;
      if (!seen.has(entry)) queue.push(entry);
    }
  }
  return seen;
}

function check(expected, resolved) {
  const a = [...expected].sort();
  const b = [...resolved].sort();
  const missing = a.filter((x) => !resolved.has(x));
  const extra = b.filter((x) => !expected.has(x));
  return { ok: missing.length === 0 && extra.length === 0, expected: a, resolved: b, missing, extra };
}

function report(title, result) {
  console.log(title);
  console.log("  声明+default 闭包 = " + JSON.stringify(result.expected));
  console.log("  cargo 实际激活  = " + JSON.stringify(result.resolved));
  if (result.ok) {
    console.log("  ✅ 一致");
  } else {
    console.log("  ❌ 不一致");
    if (result.missing.length > 0) console.log("    声明了但未激活: " + JSON.stringify(result.missing));
    if (result.extra.length > 0) console.log("    激活了但未声明: " + JSON.stringify(result.extra));
  }
}

const expected = closure(declared, usesDefault);
const resolved = new Set(node.features ?? []);
const actual = check(expected, resolved);
report("jieba-rs feature 集合（声明 vs 解析）", actual);
if (!actual.ok) {
  console.error("❌ workspace 声明的 jieba-rs feature 与 cargo 实际解析不一致（见上两侧）");
  process.exit(1);
}

if (selfTest) {
  // 负对照：故意把期望侧多塞一个 feature，校验器必须能抓出来
  const mutated = new Set(expected);
  mutated.add("tfidf");
  const fake = check(mutated, resolved);
  if (fake.ok) fail("自检失败：注入 tfidf 后校验器仍报一致");
  console.log("  ✅ 自检：注入一个不存在的声明 feature 后校验器正确报不一致（missing=" + JSON.stringify(fake.missing) + "）");
}