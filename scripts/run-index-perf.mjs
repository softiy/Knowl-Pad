// 索引与改写性能基线：把三个**后端实测**（AC-FILE-06 列表、DEBT-04 裁决、NFR-PERF-16 改写）纳入门禁。
//
// 为什么单独一个脚本：门禁 10（run-perf.mjs）测的是**前端渲染**，索引路径的耗时在 Rust 侧，
// 只能靠跑测试并解析它们的打印值 —— 这正是独立审查指出的"门禁 10 零索引指标"的补法。
//
// 语义与门禁 10 一致：预算 + 2× 跨机容忍；**基线缺失直接 exit 2**（不许自愈成"通过"）。
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname } from "node:path";
import process from "node:process";

const BASELINE = "perf/index-baseline.json";
const TOLERANCE = 2.0;

// 绝对噪声下限（毫秒）—— **小基数指标必须靠它**。
//
// 只有比例没有绝对值时：基线 2 ms 的 fileTreeSubMs 实测 5 ms 就是 250%（绝对差仅 3 ms），
// 完全在调度噪声内，门禁却会红。2026-10-12 实测发生过（#112 的 17 项门禁因此失败），
// 而同一提交在别的 runner 上是绿的。
//
// 危害不只是"偶发红"：维护者会养成"重跑一下就好"的习惯，于是**真正的回退**也被重跑掩盖 ——
// 门禁的可信度就是这样没的。真回退（例如 2 ms → 50 ms，差 48 ms）依然会被下面两条判定拦住。
const ABS_FLOOR_MS = 15;
const UPDATE = process.argv.includes("--update-baseline");

function runTest(filter, pkg = "knowl-pad") {
  // DEP0190：不要把 args 数组和 shell:true 混用（Node 会警告参数未转义）。
  // 与 run-security.mjs 同样处理：拼成单条命令字符串再交给 shell。
  //
  // pkg 参数：改写性能用例在 **kp-domain** 的集成测试里（crates/kp-domain/tests/link_rewrite_perf.rs），
  // 不在 knowl-pad 这个 bin 里 —— 写死包名会让 filter 匹配不到任何东西（实测踩过）。
  const line = ["cargo", "test", "-p", pkg, "--locked", "--", "--nocapture", filter].join(" ");
  const res = spawnSync(line, { stdio: ["ignore", "pipe", "pipe"], encoding: "utf8", shell: true });
  return (res.stdout || "") + (res.stderr || "");
}

function num(re, text, label) {
  const m = text.match(re);
  if (!m) {
    console.error("❌ 未能从输出解析出 " + label + "（测试是否被改名或不再打印？）");
    process.exit(1);
  }
  return Number(m[1]);
}

console.log("▶ 运行后端索引性能用例（--nocapture 取实测值）");
const outFileTree = runTest("ac_file_06_listing_scales");
const outResolve = runTest("debt_04_link_candidates");
// 审计 T-30：此前只覆盖索引侧（列表 + 裁决），**链接改写侧**的墙钟无人消费 ——
// 而它恰恰是 NFR-PERF-16 的正面指标（改名 + 改写 300 处引用 ≤ 3s/5s/15s）。
// 那个用例本来就打印实测值（round 13 起"只打印、不断言"），这里把它接进基线。
const outRewrite = runTest("nfr_perf_16_rewrites_three_hundred", "kp-domain");

const measured = {
  // 10050 个文件下：根目录列举 / 子目录列举（AC-FILE-06 后端侧）
  fileTreeRootMs: num(/根目录列举 (\d+) ms/, outFileTree, "根目录列举耗时"),
  fileTreeSubMs: num(/子目录 (\d+) ms/, outFileTree, "子目录列举耗时"),
  // 10 万文件 / 1 万链接的链接裁决（DEBT-04）
  resolveLinksMs: num(/裁决耗时 (\d+) ms/, outResolve, "裁决耗时"),
  // 300 处引用的自动改写总耗时（NFR-PERF-16 的正面指标；解析 link_rewrite_perf 的打印值）
  rewrite300Ms: num(/NFR-PERF-16 实测：计划 \d+ ms \+ 落地 \d+ ms = (\d+) ms/, outRewrite, "300 处改写总耗时"),
};

for (const [k, v] of Object.entries(measured)) console.log("  " + k + " = " + v + " ms");

if (!existsSync(BASELINE) && !UPDATE) {
  // **不许自愈成"通过"**（本文件第 6 行的承诺）：基线缺失时用当前实测重建，
  // 等于把"可能已经回退的性能"当成新标准放行 —— 审计 T-29 指出这里与注释正面矛盾。
  console.error("❌ 缺少基线文件 " + BASELINE + "：请用 --update-baseline 显式生成并提交。");
  process.exit(2);
}

if (UPDATE) {
  mkdirSync(dirname(BASELINE), { recursive: true });
  writeFileSync(BASELINE, JSON.stringify(measured, null, 2) + "\n", "utf8");
  console.log((UPDATE ? "✅ 基线已更新：" : "✅ 首次生成基线：") + BASELINE);
  process.exit(0);
}

const baseline = JSON.parse(readFileSync(BASELINE, "utf8"));
let regressed = 0;
for (const [k, v] of Object.entries(measured)) {
  const base = baseline[k];
  if (typeof base !== "number") {
    console.error("❌ 基线里缺少 " + k + "：请用 --update-baseline 重新生成");
    process.exit(1);
  }
  const ratio = base === 0 ? (v === 0 ? 1 : TOLERANCE + 1) : v / base;
  const delta = v - base;
  // 判定 = 比例超容忍 **且** 绝对差达到下限（两者同时成立才算回退）。
  const ok = ratio <= TOLERANCE || delta < ABS_FLOOR_MS;
  console.log(
    "  " + (ok ? "✅" : "❌") + " " + k + "：" + v + " ms（基线 " + base + " ms，" + (ratio * 100).toFixed(1) + "%，绝对差 " + delta + " ms）",
  );
  if (!ok) regressed += 1;
}

if (regressed > 0) {
  console.error("❌ 索引性能回退：" + regressed + " 项超过 " + TOLERANCE + "× 容忍");
  process.exit(1);
}
console.log("✅ 索引性能在预算内");
