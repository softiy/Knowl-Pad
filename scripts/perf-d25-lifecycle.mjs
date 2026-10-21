#!/usr/bin/env node
// NFR-PERF-13 / NFR-PERF-14 的 **App 生命周期采样**（D-25 ④⑤，2026-10-21）。
//
// 为什么需要这个脚本：内存与空闲 CPU 只能对**真实进程树**（主进程 + WebView2 子进程）采样，
// 而 `scripts/sample-process.mjs` 只给"给定 PID 的采样"——这里补上"启动 → 空闲 → 采样 → 退出 → 核对已退出"的生命周期。
//
// 安全约束（裁定要求，全部由本脚本强制）：
//   ① 只采样 `--exe` 指定的构建产物（默认 target/debug/knowl-pad.exe = 调试构建，才认 KP_CONFIG_DIR）；
//   ② 配置目录与 Vault 全部在 %TEMP% 下的一次性目录里 —— **绝不**打开冒烟 Vault，也**绝不**碰真实配置目录；
//   ③ 采样前若已有 knowl-pad 进程在跑，直接拒绝（否则进程树会混入别人的窗口/索引）；
//   ④ 退出后用 `Get-Process` 核对进程确实已消失，打印结论。
//
// **止点（如实登记进真源）**：没有 UI 自动化设施 ⇒ 采样的是"启动后停在 Vault 列表"的空闲态；
// 「编辑 5 篇后的常驻内存」是**真机项**，本脚本不冒充它。空闲 CPU 的口径（无操作 30s 后起测）由 --seconds 控制。
//
// 用法：node scripts/perf-d25-lifecycle.mjs [--exe target/debug/knowl-pad.exe] [--seconds 30] [--json out.json]
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const exe = path.resolve(arg("--exe", path.join("target", "debug", "knowl-pad.exe")));
const seconds = Number(arg("--seconds", "30"));
const jsonOut = arg("--json", "");
if (!existsSync(exe)) { console.error("❌ 找不到可执行文件：" + exe); process.exit(2); }

const ps = (script) =>
  execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" }).trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ① 前置检查：不得有别的 knowl-pad 在跑
const running = ps("@(Get-Process -Name knowl-pad -ErrorAction SilentlyContinue).Count");
if (running !== "0") {
  console.error("❌ 已有 knowl-pad 进程在运行（" + running + " 个）：请先退出，再采样（避免进程树被他人的窗口污染）");
  process.exit(2);
}

// ② 隔离目录：全部在 %TEMP%（不碰冒烟 Vault、不碰真实配置目录）
const root = mkdtempSync(path.join(os.tmpdir(), "kp-d25-life-"));
const cfg = path.join(root, "config");
const vault = path.join(root, "vault");
mkdirSync(cfg, { recursive: true });
mkdirSync(vault, { recursive: true });
for (let i = 0; i < 30; i += 1) {
  writeFileSync(path.join(vault, `note-${i}.md`), `# 笔记 ${i}\n\n内容 [[note-0]] #采样/测试\n`);
}
console.log("隔离根目录 = " + root);
console.log("KP_CONFIG_DIR = " + cfg + "（仅调试构建生效）");

// ③ 启动（继承环境 + 覆盖配置目录）
const child = spawn(exe, [], { env: { ...process.env, KP_CONFIG_DIR: cfg }, stdio: "ignore" });
const pid = child.pid;
if (!pid) { console.error("❌ 启动失败"); process.exit(1); }
console.log("已启动：pid=" + pid);

// ④ 等应用初始化存储（配置目录里出现 global.db）再等 5s 让启动抖动过去
const globalDb = path.join(cfg, "global.db");
const waitStarted = Date.now();
while (!existsSync(globalDb) && Date.now() - waitStarted < 30000) {
  await sleep(500);
}
if (!existsSync(globalDb)) {
  console.error("❌ 30s 内没有创建 " + globalDb + "（KP_CONFIG_DIR 未生效？是否在用 release 构建？）");
  process.exit(1);
}
console.log("配置目录已初始化（global.db 已创建），等待 5s 稳定");
await sleep(5000);

// ⑤ 采样进程树（同一套 D25 口径）
const sample = execFileSync(process.execPath, [path.join("scripts", "sample-process.mjs"), "--pid", String(pid), "--seconds", String(seconds), "--interval", "1", "--tree"], { encoding: "utf8" });
const lines = sample.split(/\r?\n/).filter((l) => l.startsWith("D25 "));
for (const l of lines) console.log(l);

// ⑥ 退出：先优雅（WM_CLOSE），10s 不退则强制；随后核对进程确实已消失
try { execFileSync("taskkill", ["/PID", String(pid)], { stdio: "ignore" }); } catch { /* 已退出时 taskkill 会失败，下面统一核对 */ }
for (let i = 0; i < 20; i += 1) {
  const alive = ps("@(Get-Process -Id " + pid + " -ErrorAction SilentlyContinue).Count");
  if (alive === "0") break;
  await sleep(500);
}
let alive = ps("@(Get-Process -Id " + pid + " -ErrorAction SilentlyContinue).Count");
if (alive !== "0") {
  console.log("优雅退出超时，强制结束 pid=" + pid);
  try { execFileSync("taskkill", ["/PID", String(pid), "/F"], { stdio: "ignore" }); } catch { /* 忽略 */ }
  await sleep(1500);
  alive = ps("@(Get-Process -Id " + pid + " -ErrorAction SilentlyContinue).Count");
}
const leftovers = ps("@(Get-Process -Name knowl-pad -ErrorAction SilentlyContinue).Count");
console.log("进程核对：目标 pid 仍存活 = " + (alive !== "0") + "；残留 knowl-pad 进程数 = " + leftovers);
if (alive !== "0" || leftovers !== "0") { console.error("❌ 采样结束后仍有 knowl-pad 进程"); process.exit(1); }

// ⑦ 汇总（内存为**双判定**：Windows 平台档 500MB 见勘误 D-45，PRD 原标准档 300MB 仍须打印；空闲 CPU < 1%）
const summary = { exe, seconds, isolatedRoot: root, samples: lines };
if (jsonOut) { writeFileSync(jsonOut, JSON.stringify(summary, null, 2) + "\n"); console.log("已写 " + jsonOut); }
const mem = lines.find((l) => l.includes("NFR-PERF-13-memory"));
const cpu = lines.find((l) => l.includes("NFR-PERF-14-idle-cpu"));
const mb = mem ? Number(/mb=([0-9.]+)/.exec(mem)[1]) : NaN;
const pct = cpu ? Number(/percent=([0-9.]+)/.exec(cpu)[1]) : NaN;
// 预算从 perf/d25-budgets.json 读（单一来源）；平台档 = 勘误 D-45 的 Windows/WebView2 判定阈值
const budgets = JSON.parse(readFileSync(new URL("../perf/d25-budgets.json", import.meta.url), "utf8"));
const memBudget = budgets.budgets.find((b) => b.id === "NFR-PERF-13-memory") ?? {};
const platformMb = Number(memBudget.budget_windows ?? 500);
const prdMb = Number(memBudget.budget?.standard ?? 300);
console.log("机器前提：cpus=" + os.cpus().length + " platform=" + process.platform + "（数字只在本机可比）");
// 双判定（勘误 D-45）：平台档达标 ≠ 原口径达标 —— 两个都必须打印，禁止只给一个绿
console.log("判定（Windows 平台档，勘误 D-45，预算 " + platformMb + " MB）：" + mb.toFixed(1) + " MB → " + (mb < platformMb ? "✅ 达标" : "❌ 超预算"));
console.log("判定（PRD 原标准档，预算 " + prdMb + " MB）：" + mb.toFixed(1) + " MB → " + (mb < prdMb ? "✅ 达标" : "❌ 不达标（原口径仍未达标）"));
console.log("判定（PRD）：空闲 CPU " + pct.toFixed(3) + " % / 预算 1 % → " + (pct < 1 ? "✅ 达标" : "❌ 超预算"));
console.log("止点：本次是「启动后停在 Vault 列表」的空闲态采样；「编辑 5 篇后的常驻内存」为真机项（无 UI 自动化）。");