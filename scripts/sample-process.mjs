#!/usr/bin/env node
// 进程级采样器（M5 批次 2/4）：给定 PID，采样**常驻内存**与 **CPU 占用**。
//
// 口径（PRD §6.1.4，勘误 D-27）：内存与 CPU 一律按**进程树合计**测量 ——
// Tauri 应用在 Windows 上会另起 WebView2 子进程、在 Linux 上会有 WebKit 子进程，只测主进程会漏掉大头。
//
// 为什么用 PowerShell 而不是新依赖：Node 只能读自己的内存；跨进程要系统 API ——
// Windows 上 `Get-Process` 给 WorkingSet64 / TotalProcessorTime，`Get-CimInstance Win32_Process` 给父子关系，零依赖、可审计。
//
// 用法：node scripts/sample-process.mjs --pid <PID> [--seconds 30] [--interval 1] [--tree]
import { execFileSync } from "node:child_process";
import process from "node:process";

const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const pid = Number(arg("--pid", "0"));
const seconds = Number(arg("--seconds", "30"));
const interval = Number(arg("--interval", "1"));
const TREE = argv.includes("--tree");
if (!pid) {
  console.error("❌ 需要 --pid");
  process.exit(2);
}

const ps = (script) => execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" }).trim();

// 单进程：直接取该 PID 的内存与 CPU 时间。
const oneProc = "$p = Get-Process -Id " + pid + " -ErrorAction SilentlyContinue; if ($p) { \"$($p.WorkingSet64) $($p.TotalProcessorTime.TotalMilliseconds) 1\" }";
// 进程树：从 root 出发按 ParentProcessId 迭代闭包，再对闭包求和。
const treeProcs = [
  "$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId;",
  "$ids = @(" + pid + "); $changed = $true;",
  "while ($changed) { $changed = $false; foreach ($p in $all) { if (($ids -contains $p.ParentProcessId) -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $changed = $true } } };",
  "$sum = 0; $cpu = 0; foreach ($id in $ids) { $proc = Get-Process -Id $id -ErrorAction SilentlyContinue; if ($proc) { $sum += $proc.WorkingSet64; $cpu += $proc.TotalProcessorTime.TotalMilliseconds } };",
  "if ($ids.Count -gt 0) { \"$sum $cpu $($ids.Count)\" }",
].join(" ");

const cpuCount = Number(process.env.NUMBER_OF_PROCESSORS ?? "4");
let first = null;
let last = null;
let procs = 0;
const rssSamples = [];
const started = Date.now();
while (Date.now() - started < seconds * 1000) {
  const line = ps(TREE ? treeProcs : oneProc);
  if (!line) { console.error("❌ 进程 " + pid + " 不存在（已退出？）"); process.exit(1); }
  const [ws, cpuMs, n] = line.split(/\s+/).map(Number);
  const sample = { t: Date.now() - started, ws, cpuMs };
  rssSamples.push(ws);
  procs = n;
  if (!first) first = sample;
  last = sample;
  await new Promise((r) => setTimeout(r, interval * 1000));
}

const mbPeak = Math.max(...rssSamples) / (1024 * 1024);
const mbLast = rssSamples[rssSamples.length - 1] / (1024 * 1024);
const wallMs = Math.max(1, last.t - first.t);
const cpuPercent = ((last.cpuMs - first.cpuMs) / wallMs / cpuCount) * 100;
const scope = TREE ? "tree(" + procs + " procs)" : "single";
console.log("D25 NFR-PERF-13-memory mb=" + mbLast.toFixed(1) + " peak_mb=" + mbPeak.toFixed(1) + " samples=" + rssSamples.length + " scope=" + scope);
console.log("D25 NFR-PERF-14-idle-cpu percent=" + cpuPercent.toFixed(3) + " window_ms=" + wallMs + " cpus=" + cpuCount + " scope=" + scope);
