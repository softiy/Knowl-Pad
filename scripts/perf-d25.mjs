#!/usr/bin/env node
// D-25 采样/汇总器（M5 批次「性能设施」）。
//
// 分工：本脚本**只做编排与汇总** —— 度量由 Rust 侧的用例完成（它们调用的就是生产入口），
// 打印统一格式的行：`D25 <判据 ID> key=value ...`；本脚本解析这些行、与 perf/d25-budgets.json 的
// PRD 预算比对、写出 perf/d25-report.json，并打印**覆盖表**（哪条已可测、哪条仍待接入、归谁）。
//
// 为什么不做成门禁：门禁编号是 PRD §8.4 的固定契约（17 项）。本脚本先作为"可重复运行的采样器"存在，
// 待 D-25 的判据全部接入后再讨论是否提升为门禁（见 M5 计划 §7 批次 5）。
//
// **不许自愈**：未接入的判据一律显式打印为"待接入"，绝不因为"没测到"就当作通过。
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import process from "node:process";

const BUDGETS = "perf/d25-budgets.json";
const REPORT = "perf/d25-report.json";
const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const tier = arg("--tier", "standard");
const WRITE = !argv.includes("--no-write");

if (!existsSync(BUDGETS)) {
  console.error("❌ 缺少 " + BUDGETS);
  process.exit(2);
}
const spec = JSON.parse(readFileSync(BUDGETS, "utf8"));

// 一次跑完所有 D25 用例（同一个测试二进制，避免多次编译/多进程干扰墙钟）。
//
// ⚠️ `--test-threads=1` 是**必须**的（2026-10-15 M5 批次 5 实测发现）：
//   ① 多个 d25 用例会索引同一个夹具目录，并行时互相踩库 —— 实测两个用例同时全量索引会让
//      `full_index_cancellable` 直接失败（不是"慢"，是错）；
//   ② 墙钟类指标在并行下会被同机竞争污染，测出来的数字不能作为预算判据。
console.log("▶ 运行 D-25 度量用例（tier=" + tier + "，单线程）");
const line = ["cargo", "test", "-p", "knowl-pad", "--locked", "--", "--nocapture", "--test-threads=1", "d25_"].join(" ");
const res = spawnSync(line, { stdio: ["ignore", "pipe", "pipe"], encoding: "utf8", shell: true, env: { ...process.env, KP_D25_TIER: tier } });
const out = (res.stdout || "") + (res.stderr || "");

// 解析 `D25 <ID> key=value ...`
const measured = new Map();
for (const l of out.split(/\r?\n/)) {
  const m = l.match(/D25\s+([A-Za-z0-9-]+)\s+(.*)$/);
  if (!m) continue;
  const kv = {};
  for (const p of m[2].trim().split(/\s+/)) { const [k, v] = p.split("="); if (k) kv[k] = v === "true" ? true : v === "false" ? false : isNaN(Number(v)) ? v : Number(v); }
  measured.set(m[1], kv);
}

if (res.status !== 0 && measured.size === 0) {
  console.error("❌ 度量用例没有产出任何 D25 行（exit=" + res.status + "）。原始输出尾部：");
  console.error(out.split(/\r?\n/).slice(-25).join("\n"));
  process.exit(1);
}

function budgetOf(item) {
  if (!item.budget) return null;
  return typeof item.budget === "object" ? item.budget[tier] ?? null : item.budget;
}

function valueOf(item, kv) {
  // 约定：判据 ID 去掉 -<后缀> 后的主键名用于取值（例如 NFR-PERF-03-first-index → ms）
  for (const k of ["ms", "mb", "percent", "events", "ok"]) if (k in kv) return kv[k];
  return null;
}

const report = { tier, generatedAt: new Date().toISOString(), items: [], coverage: { instrumented: 0, pending: 0, failed: 0 } };
let failed = 0;
for (const item of spec.budgets) {
  const kv = measured.get(item.id);
  const b = budgetOf(item);
  // 手动脚本（审查 M7）：有测量方法、但**不经过本采样器**（例如 App 生命周期采样）。
  // 显式标成「手动脚本 + 命令」，不写成「待接入」（那会和 instrumented:true 自相矛盾）。
  if (item.manual) {
    report.items.push({ id: item.id, status: "manual", command: item.manual, owning: item.owning });
    report.coverage.manual = (report.coverage.manual ?? 0) + 1;
    console.log("  🖐  " + item.id.padEnd(30) + " 手动脚本（不自动判定）：" + item.manual);
    continue;
  }
  if (!kv) {
    report.items.push({ id: item.id, status: "pending", plan: item.plan, owning: item.owning });
    report.coverage.pending += 1;
    console.log("  ⏳ " + item.id.padEnd(26) + " 待接入 —— " + item.plan);
    continue;
  }
  const v = valueOf(item, kv);
  if (item.kind === "proxy") {
    report.items.push({ id: item.id, status: "proxy", value: v, unit: item.unit, raw: kv, owning: item.owning });
    report.coverage.proxy = (report.coverage.proxy ?? 0) + 1;
    console.log("  🔬 " + item.id.padEnd(32) + " " + v + " " + item.unit + "（proxy，不计入判优）");
    continue;
  }
  // 无预算的验收判据：以 kv.ok === 1 或 cancel-observed=true 之类的显式信号为准
  const okSignal = kv.ok === 1 || kv.ok === true || kv["cancel-observed"] === true;
  const ok = b === null ? okSignal : typeof v === "number" && v <= b;
  const shown = b === null ? (okSignal ? "信号成立" : "信号不成立") : v;
    report.items.push({ id: item.id, status: ok ? "instrumented-ok" : "instrumented-fail", value: v, unit: item.unit, budget: b, raw: kv, owning: item.owning });
  if (ok) { report.coverage.instrumented += 1; console.log("  ✅ " + item.id.padEnd(28) + " " + (typeof shown === "string" ? shown : v + " " + item.unit + "（预算 " + b + "）")); }
  else { report.coverage.failed += 1; failed += 1; console.log("  ❌ " + item.id.padEnd(28) + " " + (b === null ? "信号不成立（" + JSON.stringify(kv) + "）" : v + " " + item.unit + " 超出预算 " + b)); }
}

console.log("\n▶ D-25 覆盖：" + report.coverage.instrumented + " 已达标 / " + report.coverage.failed + " 超预算 / " + (report.coverage.proxy ?? 0) + " proxy / " + (report.coverage.manual ?? 0) + " 手动脚本 / " + report.coverage.pending + " 待接入（共 " + spec.budgets.length + " 条，proxy 不计入判优）");
if (WRITE) {
  writeFileSync(REPORT, JSON.stringify(report, null, 2) + "\n", "utf8");
  console.log("  报告已写入 " + REPORT);
}
process.exit(failed > 0 ? 1 : 0);
