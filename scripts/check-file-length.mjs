#!/usr/bin/env node
// CODE-11：单文件行数限制（警告 200 / 硬性上限 350）——**Rust 与前端一视同仁**
// （PRD 原文：「Rust 文件同样适用」；2026-10-05 由 M2 PR-9 把扫描范围扩到 src/**）。
//
// PRD CODE-11 原文：超警告阈值报 warning；超硬性上限 CI 报 error 并拒绝合并；
// 「测试文件与 schema.rs（DDL 定义）豁免硬性上限」。
// 对齐说明：豁免在**实现上对两级都生效**（measure() 里 exempt 的文件 level 恒为 "ok"，
// 既不报 warning 也不报 error）。此前注释只提"硬性上限"，比实现窄 —— 这里与实现对齐，
// 免得读的人以为测试文件还会被预警计数（它们不计入 WARN_BUDGET）。
//
// 豁免规则（本脚本的口径）：
//   - 路径含 /tests/ 或文件名以 _tests.rs / _test.rs 结尾（测试文件）
//   - 文件名以 schema.rs 结尾（DDL 定义，如 index_schema.rs）
//   - 前端：*.spec.ts（测试）与 *.d.ts（类型声明）
// 用法：node scripts/check-file-length.mjs [--self-test]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { pathToFileURL } from "node:url";
import process from "node:process";

// 只在"被当作脚本直接运行"时执行扫描：作为模块 import（自检/演示/未来的测试）时不得有副作用
// —— 此前 import 会直接跑完整扫描并可能 process.exit(1)，既无法演示也无法单测。
const RUN_AS_SCRIPT = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
const ROOT = process.cwd();
const SCAN_DIRS = [join(ROOT, "src-tauri", "src"), join(ROOT, "crates"), join(ROOT, "src")];
const SCAN_EXTENSIONS = [".rs", ".ts", ".vue"];
const WARN_LIMIT = 200;
const HARD_LIMIT = 350;

export function isExempt(relPath) {
  const normalized = relPath.replace(/\\/g, "/");
  return (
    normalized.includes("/tests/") ||
    /_tests?\.rs$/.test(normalized) ||
    /schema\.rs$/.test(normalized) ||
    /\.spec\.ts$/.test(normalized) ||
    /\.d\.ts$/.test(normalized)
  );
}

export function measure(relPath, content) {
  // 物理行数（与 wc -l 同口径）：末尾换行不计为额外一行
  const newlines = (content.match(/\n/g) ?? []).length;
  const lines = content.endsWith("\n") ? newlines : newlines + 1;
  const exempt = isExempt(relPath);
  let level = "ok";
  if (lines > HARD_LIMIT && !exempt) level = "error";
  else if (lines > WARN_LIMIT && !exempt) level = "warn";
  return { lines, exempt, level };
}

// DEBT-21：警告预算 —— **只许降不许升**；要放宽必须在本文件里改这个数字，并在 PR 说明理由
// （让"临时放一马"留下痕迹）。注意它**不是**命令行参数：可运行时覆盖就等于可绕过。
export const WARN_BUDGET = 22;

/**
 * 把检查结果渲染成**有序**的输出行（纯函数，可自检）。
 *
 * 为什么要有这个函数（2026-10-13 的真实教训）：此前主流程在"超预算"时**先 exit(1)**、
 * 之后才打印逐文件清单 —— 于是门禁只在 CI 上留下一句"23 超过预算 22"，
 * **看不到是哪个文件**。当时为定位它，我不得不另写脚本按同样口径重算一遍。
 * 顺序在这里被固化成契约：**逐文件清单永远先于结论行**；
 * 且无论通过与否，清单都在（通过时也要能看见"哪些文件快到线了"）。
 */
export function renderReport(results, budget = WARN_BUDGET) {
  const warns = results.filter((r) => r.level === "warn");
  const errors = results.filter((r) => r.level === "error");
  const lines = [
    {
      stream: "info",
      kind: "header",
      text:
        "▶ CODE-11 文件长度检查：" +
        results.length +
        " 个源文件（Rust + 前端；豁免 " +
        results.filter((r) => r.exempt).length +
        " 个）",
    },
  ];
  // ↓ 逐文件清单：**必须**在结论行之前，且不因失败而省略
  for (const w of warns) {
    lines.push({ stream: "warn", kind: "file", text: "⚠️  " + w.rel + " 为 " + w.lines + " 行（警告阈值 " + WARN_LIMIT + "）" });
  }
  for (const e of errors) {
    lines.push({ stream: "error", kind: "file", text: "❌ " + e.rel + " 为 " + e.lines + " 行，超过硬性上限 " + HARD_LIMIT });
  }
  // ↓ 结论行
  let failed = false;
  if (warns.length > budget) {
    failed = true;
    lines.push({
      stream: "error",
      kind: "verdict",
      text: "❌ 超警告阈值的文件数 " + warns.length + " 超过预算 " + budget + "：请拆分文件或调整预算并说明理由",
    });
  }
  if (errors.length > 0) {
    failed = true;
    lines.push({ stream: "error", kind: "verdict", text: "❌ CODE-11 失败：" + errors.length + " 个文件超限" });
  }
  if (!failed) {
    lines.push({ stream: "info", kind: "verdict", text: "✅ CODE-11 通过（超硬性上限 0 个，超警告阈值 " + warns.length + " 个）" });
  }
  return { lines, failed, warns: warns.length, errors: errors.length };
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
  const cases = [
    ["src-tauri/src/a.rs", 351, "error"],
    ["src-tauri/src/a.rs", 201, "warn"],
    ["src-tauri/src/a.rs", 200, "ok"],
    ["crates/kp-domain/tests/big.rs", 900, "ok"],
    ["src-tauri/src/storage/index_schema.rs", 900, "ok"],
    ["src-tauri/src/commands/vault_tests.rs", 900, "ok"],
    // 前端（2026-10-05 起纳入同一门禁）
    ["src/features/x.ts", 351, "error"],
    ["src/features/x.ts", 201, "warn"],
    ["src/features/x.ts", 200, "ok"],
    ["src/App.vue", 351, "error"],
    ["src/core/ipc/commands.spec.ts", 900, "ok"],
    ["src/env.d.ts", 900, "ok"],
  ];

  let pass = 0;
  for (const [path, lines, want] of cases) {
    const content = Array.from({ length: lines }, () => "x").join("\n") + "\n";
    const got = measure(path, content).level;
    if (got === want) pass += 1;
    else console.error(`❌ 自检失败：${path} ${lines} 行 期望 ${want} 实得 ${got}`);
  }
  // renderReport 的顺序契约：**逐文件清单先于结论行**，且失败时清单不得缺席
  const mk = (levels) =>
    levels.map((level, i) => ({ rel: "src/f" + i + ".ts", lines: level === "error" ? 360 : 210, level, exempt: false }));
  const overBudget = renderReport(mk(Array.from({ length: WARN_BUDGET + 1 }, () => "warn")));
  const atBudget = renderReport(mk(Array.from({ length: WARN_BUDGET }, () => "warn")));
  const withError = renderReport(mk(["error"]));
  // 顺序契约用**显式的 kind** 判定 —— 不能靠"以 ❌ 开头"这类启发式：
  // 逐文件的 error 行同样以 ❌ 开头，我第一版就是这么把两条用例判错的。
  const orderOk = (rep) => {
    const lastFile = rep.lines.map((l) => l.kind).lastIndexOf("file");
    const firstVerdict = rep.lines.findIndex((l) => l.kind === "verdict");
    return lastFile >= 0 && firstVerdict >= 0 && lastFile < firstVerdict;
  };
  const reportCases = [
    ["每行都带 kind（顺序契约的依据）", overBudget.lines.every((l) => ["header", "file", "verdict"].includes(l.kind))],
    ["超预算必须失败", overBudget.failed === true],
    ["超预算时仍打印逐文件清单", overBudget.lines.some((l) => l.text.startsWith("⚠️"))],
    ["逐文件清单先于结论行", orderOk(overBudget)],
    ["恰好等于预算不失败", atBudget.failed === false],
    ["通过时也打印清单", atBudget.lines.some((l) => l.text.startsWith("⚠️"))],
    ["硬性上限失败", withError.failed === true],
    ["硬性上限也先列文件", orderOk(withError)],
  ];
  for (const [name, okCase] of reportCases) {
    if (okCase) pass += 1;
    else console.error("❌ 自检失败：" + name);
  }
  console.log(`自检：${pass}/${cases.length + reportCases.length} 通过`);
  process.exit(pass === cases.length + reportCases.length ? 0 : 1);
}

if (RUN_AS_SCRIPT) {
  // DEBT-21：此前是 `catch { return [] }` —— 扫描目录一旦失败（改名/权限/路径写错）会**静默退化为零文件**，
  // 门禁照样"通过"。现在：① 异常直接抛出；② 校验扫描到的文件数下限，防止"空扫描 = 通过"。
  const MIN_FILES = 50;
  const files = SCAN_DIRS.flatMap((dir) => walk(dir));
  if (files.length < MIN_FILES) {
    console.error(
      "❌ CODE-11 扫描到的文件过少（" + files.length + " < " + MIN_FILES + "）：扫描目录是否被改动或不可读？",
    );
    process.exit(1);
  }
  const results = files.map((f) => {
    const rel = relative(ROOT, f);
    return { rel, ...measure(rel, readFileSync(f, "utf8")) };
  });

  const report = renderReport(results);
  for (const line of report.lines) {
    if (line.stream === "error") console.error(line.text);
    else if (line.stream === "warn") console.warn(line.text);
    else console.log(line.text);
  }
  if (report.failed) process.exit(1);
}
