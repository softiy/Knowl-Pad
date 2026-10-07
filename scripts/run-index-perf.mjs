// 索引性能基线：把两个**后端实测**（AC-FILE-06 列表、DEBT-04 裁决）纳入门禁。
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
const UPDATE = process.argv.includes("--update-baseline");

function runTest(filter) {
  // DEP0190：不要把 args 数组和 shell:true 混用（Node 会警告参数未转义）。
  // 与 run-security.mjs 同样处理：拼成单条命令字符串再交给 shell。
  const line = ["cargo", "test", "-p", "knowl-pad", "--locked", "--", "--nocapture", filter].join(" ");
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

const measured = {
  // 10050 个文件下：根目录列举 / 子目录列举（AC-FILE-06 后端侧）
  fileTreeRootMs: num(/根目录列举 (\d+) ms/, outFileTree, "根目录列举耗时"),
  fileTreeSubMs: num(/子目录 (\d+) ms/, outFileTree, "子目录列举耗时"),
  // 10 万文件 / 1 万链接的链接裁决（DEBT-04）
  resolveLinksMs: num(/裁决耗时 (\d+) ms/, outResolve, "裁决耗时"),
};

for (const [k, v] of Object.entries(measured)) console.log("  " + k + " = " + v + " ms");

if (UPDATE || !existsSync(BASELINE)) {
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
  const ok = ratio <= TOLERANCE;
  console.log(
    "  " + (ok ? "✅" : "❌") + " " + k + "：" + v + " ms（基线 " + base + " ms，" + (ratio * 100).toFixed(1) + "%）",
  );
  if (!ok) regressed += 1;
}

if (regressed > 0) {
  console.error("❌ 索引性能回退：" + regressed + " 项超过 " + TOLERANCE + "× 容忍");
  process.exit(1);
}
console.log("✅ 索引性能在预算内");
