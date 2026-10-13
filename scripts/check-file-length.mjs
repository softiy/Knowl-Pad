#!/usr/bin/env node
// CODE-11：单文件与单函数规模限制（规范条文见 PRD §8.1 CODE-11；实现口径以本文件为准）
//
// 2026-10-13 重写，修掉此前 5 类盲点（详见 PRD §8.1 的 CODE-11 条文与 DEBT-21）：
//   ① 口径没写 → 现在**明确定为 SLOC**（非空、非纯注释行；use/import 计入），物理行只作 600 的副指标防"压成一行"；
//   ② 只有文件级 → 函数级交给 clippy::too_many_lines(100，clippy 口径本身不含注释) 与 ESLint max-lines-per-function(80)；
//   ③ 豁免粒度粗 → 测试目录**只认三处**（不再任意层级含 /tests/）、DDL 认 schema(_vN)?.rs、并支持**内容特征**（cfg(test) 尾部占比）；
//   ④ 豁免不可见 → 输出豁免明细（分类计数 + 前 10；--verbose 全量）与行内豁免明细（类别 + 理由）；
//   ⑤ 语义不可分 → 新增**行内豁免**（文件前 5 行，类别取自枚举，理由必填，另受 ≤5 预算约束）。
//
// 口径细节（必须可复算，故不做语法级解析）：
//   - 纯注释行 = 行首（允许空白）为 // 、/// 、/* 、* 、*/ 之一；行尾注释按代码计。
//   - 多行字符串、Vue 的 <template>/<style> 内容按代码计（保守：宁可算多，不可算少）。
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { pathToFileURL } from "node:url";
import process from "node:process";

// 只在"被当作脚本直接运行"时执行扫描：作为模块 import（自检/演示/测试）时不得有副作用。
const RUN_AS_SCRIPT = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
const ROOT = process.cwd();
const SCAN_DIRS = [join(ROOT, "src-tauri", "src"), join(ROOT, "crates"), join(ROOT, "src"), join(ROOT, "scripts")];
// 根目录的配置文件（单层，不递归）：门禁/构建配置本身也受同一约束
const SCAN_ROOT_FILES = ["vite.config.ts", "vitest.config.ts", "eslint.config.js", "playwright.config.ts"];
const SCAN_EXTENSIONS = [".rs", ".ts", ".vue", ".mjs", ".js"];

export const WARN_SLOC = 200;
export const HARD_SLOC = 350;
export const HARD_PHYSICAL = 600;
// 预警预算（规范 C11.2）：初始值 = 实测 × 1.2 向上取整 —— 2026-10-13 实测预警 15 个 ⇒ 18。
// **只许降不许升**；要放宽必须改这里并在 PR 说明理由。M7 完成后再实测一次并按同一公式复核。
export const WARN_BUDGET = 18;
// 行内豁免预算（规范 C11.5）：同样只降不升。
export const INLINE_EXEMPT_BUDGET = 5;
export const INLINE_ALLOW_CATEGORIES = ["state-machine", "ffi", "macro-heavy", "trait-impls"];
export const INLINE_ALLOW_MAX_REASON = 120;
export const INLINE_ALLOW_SCAN_LINES = 5;
// 内容特征豁免（规范 C11.4-4）：cfg(test) 起至文件末尾的行占比 ≥ 50% ⇒ 按测试文件处理。
export const CFG_TEST_TAIL_RATIO = 0.5;

export const isCommentLine = (line) => /^\s*(\/\/|\/\*|\*|\*\/)/.test(line);

/** SLOC = 非空行且非纯注释行（use/import 计入）。 */
export function countSloc(content) {
  return content.split("\n").filter((l) => l.trim() !== "" && !isCommentLine(l)).length;
}

export function countPhysical(content) {
  return (content.match(/\n/g) ?? []).length;
}

/** 路径豁免（规范 C11.4-1~3）。返回豁免类别或 null。 */
export function pathExemption(relPath) {
  const p = relPath.replace(/\\/g, "/");
  if (/^tests\//.test(p) || /^crates\/[^/]+\/tests\//.test(p) || /^src-tauri\/tests\//.test(p)) return "test-dir";
  if (/_tests?\.rs$/.test(p)) return "test-file";
  if (/\.spec\.ts$/.test(p)) return "test-file";
  if (/\.d\.ts$/.test(p)) return "test-file";
  if (/(^|[\/_])schema(_v\d+)?\.rs$/.test(p)) return "ddl";
  return null;
}

/** 兼容旧调用：仅看路径。新代码请用 pathExemption，或 measure(rel, content) 拿到完整判定。 */
export function isExempt(relPath) {
  return pathExemption(relPath) !== null;
}

/** 内容特征豁免：cfg(test) 起至末尾占比 ≥ 50%（保守阈值，避免"末尾附一小段测试"被整体豁免）。 */
export function cfgTestExemption(content) {
  const lines = content.split("\n");
  const i = lines.findIndex((l) => /#\[cfg\(test\)\]/.test(l));
  if (i < 0 || lines.length === 0) return null;
  return (lines.length - i) / lines.length >= CFG_TEST_TAIL_RATIO ? "cfg-test-tail" : null;
}

/** 行内豁免（规范 C11.5）：文件前 5 行内的 code11-allow-file 注释。 */
export function parseInlineAllow(content) {
  const head = content.split("\n").slice(0, INLINE_ALLOW_SCAN_LINES);
  for (const line of head) {
    const m = line.match(/code11-allow-file:\s*([A-Za-z-]+)\s*[—\-–]\s*(.*?)\s*$/);
    if (!m) continue;
    const category = m[1];
    const reason = m[2];
    if (!INLINE_ALLOW_CATEGORIES.includes(category)) {
      return { error: "未知类别 " + category + "（允许：" + INLINE_ALLOW_CATEGORIES.join(" / ") + "）" };
    }
    if (!reason) return { error: "缺少理由（理由必填）" };
    if (reason.length > INLINE_ALLOW_MAX_REASON) {
      return { error: "理由过长（" + reason.length + " > " + INLINE_ALLOW_MAX_REASON + " 字符）" };
    }
    return { category, reason };
  }
  return null;
}

/** 完整度量：SLOC 主指标 + 物理副指标 + 豁免判定 + 行内豁免。 */
export function measure(relPath, content) {
  const sloc = countSloc(content);
  const physical = countPhysical(content);
  const inline = parseInlineAllow(content);
  const byPath = pathExemption(relPath);
  const exemptKind = byPath ?? cfgTestExemption(content) ?? (inline && !inline.error ? "inline" : null);
  const exempt = exemptKind !== null;
  let level = "ok";
  if (!exempt) {
    if (sloc > HARD_SLOC || physical > HARD_PHYSICAL) level = "error";
    else if (sloc > WARN_SLOC) level = "warn";
  }
  return { sloc, physical, exempt, exemptKind, level, inlineAllow: inline };
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) walk(full, out);
    else if (SCAN_EXTENSIONS.some((ext) => name.endsWith(ext))) out.push(full);
  }
  return out;
}

if (process.argv.includes("--self-test")) {
  const levelCases = [
    ["src-tauri/src/a.rs", "x\n".repeat(351), "error"],
    ["src-tauri/src/a.rs", "// 纯注释\n".repeat(400), "ok"],       // 纯注释不计入 SLOC
    ["src-tauri/src/a.rs", "\n".repeat(700), "error"],           // 物理副指标对「空行堆砌」同样生效
    ["src-tauri/src/a.rs", "x\n".repeat(201), "warn"],
    ["src-tauri/src/a.rs", "x\n".repeat(200), "ok"],
    ["src-tauri/src/a.rs", "\n".repeat(400), "ok"],               // 空行不计入 SLOC
    ["src-tauri/src/a.rs", "x\n".repeat(601), "error"],           // 物理副指标 600
    ["src-tauri/src/a.rs", "x\n".repeat(349), "warn"],
    ["src-tauri/src/a.rs", "x\n".repeat(350), "warn"],
    ["src-tauri/src/a.rs", "x\n".repeat(351), "error"],
    ["crates/kp-domain/tests/big.rs", "x\n".repeat(900), "ok"],
    ["src-tauri/tests/helper.rs", "x\n".repeat(900), "ok"],
    ["src-tauri/src/storage/index_schema.rs", "x\n".repeat(900), "ok"],
    ["src-tauri/src/storage/schema_v2.rs", "x\n".repeat(900), "ok"],
    ["src-tauri/src/commands/vault_tests.rs", "x\n".repeat(900), "ok"],
    ["src/features/x.ts", "x\n".repeat(351), "error"],
    ["src/core/ipc/commands.spec.ts", "x\n".repeat(900), "ok"],
    ["src/env.d.ts", "x\n".repeat(900), "ok"],
  ];
  const exCases = [
    ["crates/kp-domain/tests/a.rs", true], ["src-tauri/tests/a.rs", true], ["tests/unit/a.spec.ts", true],
    ["src/features/tests/widget.ts", false],                 // 生产目录叫 tests 不再整体豁免
    ["src-tauri/src/commands/tests/helpers.rs", false],
    ["src-tauri/src/test_utils.rs", false],
    ["crates/kp-domain/src/storage/user_schema.rs", true],
    ["crates/kp-domain/src/storage/schema_patch.rs", false],
    ["scripts/run-gates.mjs", false],                        // 工具链代码同样受约束
  ];
  const tailOk = "let a = 1;\n".repeat(10) + "#[cfg(test)]\nmod tests {\n" + "x\n".repeat(30) + "}\n";
  const tailShort = "let a = 1;\n".repeat(80) + "#[cfg(test)]\nmod tests {\n" + "x\n".repeat(5) + "}\n";
  const okInline = "// code11-allow-file: state-machine — 一个 enum 与完整 match，拆开会切断可读性\nlet a = 1;\n";
  const badCat = "// code11-allow-file: whatever — 有理由\nlet a = 1;\n";
  const noReason = "// code11-allow-file: ffi —\nlet a = 1;\n";
  const lateInline = "x\n".repeat(9) + okInline;
  const miscCases = [
    ["cfg(test) 尾部占比足够 ⇒ 豁免", cfgTestExemption(tailOk) !== null],
    ["cfg(test) 尾部占比不足 ⇒ 不豁免", cfgTestExemption(tailShort) === null],
    ["行内豁免可解析", (() => { const r = parseInlineAllow(okInline); return !!r && r.category === "state-machine" && r.reason.length > 0; })()],
    ["行内豁免：未知类别被拒", (() => { const r = parseInlineAllow(badCat); return !!r && typeof r.error === "string"; })()],
    ["行内豁免：理由必填", (() => { const r = parseInlineAllow(noReason); return !!r && typeof r.error === "string"; })()],
    ["行内豁免生效（超硬上限仍判 ok）", measure("src/a.ts", okInline + "x\n".repeat(400)).level === "ok"],
    ["行内豁免只认前 5 行", parseInlineAllow(lateInline) === null],
    ["SLOC 口径：行尾注释按代码计", countSloc("let a = 1; // 说明\n") === 1],
  ];
  const reportCases = (() => {
    const mk = (levels) => levels.map((level, i) => ({ rel: "src/f" + i + ".ts", sloc: level === "error" ? 360 : 210, physical: 380, exempt: false, exemptKind: null, level, inlineAllow: null }));
    const over = renderReport(mk(Array.from({ length: WARN_BUDGET + 1 }, () => "warn")));
    const at = renderReport(mk(Array.from({ length: WARN_BUDGET }, () => "warn")));
    const hard = renderReport(mk(["error"]));
    const orderOk = (rep) => {
      const lastFile = rep.lines.map((l) => l.kind).lastIndexOf("file");
      const firstVerdict = rep.lines.findIndex((l) => l.kind === "verdict");
      return lastFile >= 0 && firstVerdict >= 0 && lastFile < firstVerdict;
    };
    return [
      ["每行都带 kind", over.lines.every((l) => ["header", "file", "info", "verdict"].includes(l.kind))],
      ["超预算必须失败", over.failed === true],
      ["超预算仍打印逐文件清单", over.lines.some((l) => l.text.startsWith("⚠️"))],
      ["逐文件清单先于结论行", orderOk(over)],
      ["恰好等于预算不失败", at.failed === false],
      ["硬上限失败", hard.failed === true],
      ["硬上限也先列文件", orderOk(hard)],
    ];
  })();
  let pass = 0;
  let total = 0;
  for (const [path, text, want] of levelCases) { total += 1; const got = measure(path, text).level; if (got === want) pass += 1; else { const mm = measure(path, text); console.error("❌ 自检失败（层级）：" + path + " 期望 " + want + " 实得 " + got + "（SLOC " + mm.sloc + "、物理 " + mm.physical + "、豁免 " + mm.exempt + "/" + mm.exemptKind + "）"); } }
  for (const [p, want] of exCases) { total += 1; if ((pathExemption(p) !== null) === want) pass += 1; else console.error("❌ 自检失败（豁免）：" + p + " 期望 " + want); }
  for (const [name, ok] of miscCases) { total += 1; if (ok) pass += 1; else console.error("❌ 自检失败：" + name); }
  for (const [name, ok] of reportCases) { total += 1; if (ok) pass += 1; else console.error("❌ 自检失败：" + name); }
  console.log("自检：" + pass + "/" + total + " 通过");
  process.exit(pass === total ? 0 : 1);
}

/** 有序输出（纯函数，可自检）：逐文件清单与豁免明细**永远先于**结论行，失败时不得省略。 */
export function renderReport(results, opts = {}) {
  const budget = opts.budget ?? WARN_BUDGET;
  const inlineBudget = opts.inlineBudget ?? INLINE_EXEMPT_BUDGET;
  const verbose = opts.verbose ?? false;
  const warns = results.filter((r) => r.level === "warn");
  const errors = results.filter((r) => r.level === "error");
  const exempts = results.filter((r) => r.exempt);
  const inlines = results.filter((r) => r.exemptKind === "inline");
  const lines = [{ stream: "info", kind: "header", text: "▶ CODE-11 文件长度检查：" + results.length + " 个源文件（口径 SLOC；豁免 " + exempts.length + " 个）" }];
  for (const w of warns) lines.push({ stream: "warn", kind: "file", text: "⚠️  " + w.rel + " 为 " + w.sloc + " SLOC（物理 " + w.physical + "；警告线 " + WARN_SLOC + "）" });
  for (const e of errors) {
    const why = e.sloc > HARD_SLOC ? e.sloc + " SLOC 超过硬性上限 " + HARD_SLOC : "物理 " + e.physical + " 行超过副指标上限 " + HARD_PHYSICAL;
    lines.push({ stream: "error", kind: "file", text: "❌ " + e.rel + "：" + why });
  }
  for (const r of inlines) lines.push({ stream: "info", kind: "info", text: "🪪 行内豁免：" + r.rel + " — " + r.inlineAllow.category + "：" + r.inlineAllow.reason });
  const byKind = {};
  for (const r of exempts) byKind[r.exemptKind] = (byKind[r.exemptKind] ?? 0) + 1;
  const kindText = Object.entries(byKind).map(([k, n]) => k + " " + n).join("、") || "无";
  lines.push({ stream: "info", kind: "info", text: "ℹ️  豁免分类：" + kindText });
  const listed = verbose ? exempts : exempts.slice(0, 10);
  for (const r of listed) lines.push({ stream: "info", kind: "info", text: "    · [" + r.exemptKind + "] " + r.rel + "（" + r.sloc + " SLOC）" });
  if (!verbose && exempts.length > listed.length) lines.push({ stream: "info", kind: "info", text: "    … 其余 " + (exempts.length - listed.length) + " 个用 --verbose 查看" });
  let failed = false;
  if (warns.length > budget) { failed = true; lines.push({ stream: "error", kind: "verdict", text: "❌ 预警文件数 " + warns.length + " 超过预算 " + budget + "：请拆分文件，或改预算并在 PR 说明理由" }); }
  if (inlines.length > inlineBudget) { failed = true; lines.push({ stream: "error", kind: "verdict", text: "❌ 行内豁免数 " + inlines.length + " 超过预算 " + inlineBudget + "（规范 C11.5）" }); }
  for (const r of results) { if (r.inlineAllow && r.inlineAllow.error) { failed = true; lines.push({ stream: "error", kind: "verdict", text: "❌ " + r.rel + " 的行内豁免写法无效：" + r.inlineAllow.error }); } }
  if (errors.length > 0) { failed = true; lines.push({ stream: "error", kind: "verdict", text: "❌ CODE-11 失败：" + errors.length + " 个文件超限" }); }
  if (!failed) lines.push({ stream: "info", kind: "verdict", text: "✅ CODE-11 通过（超硬性上限 0 个；预警 " + warns.length + "/" + budget + "；豁免 " + exempts.length + " 个，行内 " + inlines.length + "/" + inlineBudget + "）" });
  return { lines, failed, warns: warns.length, errors: errors.length, exempts: exempts.length, inlines: inlines.length };
}

if (RUN_AS_SCRIPT) {
  const MIN_FILES = 50;
  const files = SCAN_DIRS.flatMap((dir) => walk(dir));
  for (const name of SCAN_ROOT_FILES) {
    const full = join(ROOT, name);
    try { if (statSync(full).isFile()) files.push(full); } catch { /* 不存在则跳过 */ }
  }
  if (files.length < MIN_FILES) {
    console.error("❌ CODE-11 扫描到的文件过少（" + files.length + " < " + MIN_FILES + "）：扫描目录是否被改动或不可读？");
    process.exit(1);
  }
  const results = files.map((f) => { const rel = relative(ROOT, f); return { rel, ...measure(rel, readFileSync(f, "utf8")) }; });
  const report = renderReport(results, { verbose: process.argv.includes("--verbose") });
  for (const line of report.lines) {
    if (line.stream === "error") console.error(line.text);
    else if (line.stream === "warn") console.warn(line.text);
    else console.log(line.text);
  }
  if (report.failed) process.exit(1);
}
