#!/usr/bin/env node
// 进程级采样器（M5 批次 2）：给定 PID，采样**常驻内存**与**CPU 占用**。
//
// 为什么用 PowerShell 而不是新依赖：Node 只能读自己的内存；跨进程需要系统 API ——
// Windows 上 `Get-Process` 直接给出 WorkingSet64 与 TotalProcessorTime，零依赖、可审计。
// 这是 D-25「内存/空闲 CPU 采样」的落地工具；**接入对象**（真实 App 生命周期）见 perf/d25-budgets.json。
//
// 用法：node scripts/sample-process.mjs --pid <PID> --seconds 30 [--interval 1] [--label idle]
import { execFileSync } from "node:child_process";
import process from "node:process";

const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const pid = Number(arg("--pid", "0"));
const seconds = Number(arg("--seconds", "30"));
const interval = Number(arg("--interval", "1"));
if (!pid) {
  console.error("❌ 需要 --pid");
  process.exit(2);
}

const ps = (script) => {
  const out = execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
  return out.trim();
};

const cpuCount = Number(process.env.NUMBER_OF_PROCESSORS ?? "4");
let first = null;
let last = null;
const rssSamples = [];
const started = Date.now();
while (Date.now() - started < seconds * 1000) {
  const line = ps(
    "$p = Get-Process -Id " + pid + " -ErrorAction SilentlyContinue; if ($p) { \"$($p.WorkingSet64) $($p.TotalProcessorTime.TotalMilliseconds)\" }"
  );
  if (!line) { console.error("❌ 进程 " + pid + " 不存在（已退出？）"); process.exit(1); }
  const [ws, cpuMs] = line.split(/\s+/).map(Number);
  const sample = { t: Date.now() - started, ws, cpuMs };
  rssSamples.push(ws);
  if (!first) first = sample;
  last = sample;
  await new Promise((r) => setTimeout(r, interval * 1000));
}

const mbPeak = Math.max(...rssSamples) / (1024 * 1024);
const mbLast = rssSamples[rssSamples.length - 1] / (1024 * 1024);
// CPU% = ΔCPU时间 / Δ墙钟 / 核数（空闲态应当 < 1%）
const wallMs = Math.max(1, last.t - first.t);
const cpuPercent = ((last.cpuMs - first.cpuMs) / wallMs / cpuCount) * 100;
console.log("D25 NFR-PERF-13-memory mb=" + mbLast.toFixed(1) + " peak_mb=" + mbPeak.toFixed(1) + " samples=" + rssSamples.length);
console.log("D25 NFR-PERF-14-idle-cpu percent=" + cpuPercent.toFixed(3) + " window_ms=" + wallMs + " cpus=" + cpuCount);
