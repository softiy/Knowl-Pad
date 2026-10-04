#!/usr/bin/env node
// 本地等价于 PRD §8.4 的 13 项门禁；CI（.github/workflows/ci.yml）按同一顺序执行。
// 用法：node scripts/run-gates.mjs [--only <id>] [--skip <id,...>]
import { spawnSync } from 'node:child_process';

/** 为 shell 执行转义单个参数：仅在含空白或 shell 元字符时加引号。 */
function shellQuote(arg) {
  const s = String(arg);
  return /[\s"&|<>^()%!]/.test(s) ? '"' + s.replace(/"/g, '\\"') + '"' : s;
}
import { writeFileSync } from 'node:fs';
import process from 'node:process';

const GATES = [
  { id: '1', name: 'TypeScript 类型检查', cmd: ['pnpm', ['typecheck']] },
  { id: '2', name: '前端 Lint', cmd: ['pnpm', ['lint']] },
  { id: '14', name: '前端构建 + 完整性（准备 dist）', cmd: ['pnpm', ['build:verify']] },
  { id: '3', name: 'Rust Lint (clippy -D warnings)', cmd: ['cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']] },
  { id: '4', name: 'Rust 格式 (fmt --check)', cmd: ['cargo', ['fmt', '--all', '--check']] },
  { id: '6', name: 'Rust 单元/集成测试', cmd: ['cargo', ['test', '--workspace', '--locked']] },
  { id: '15', name: 'Rust domain 覆盖率 ≥ 85%', cmd: ['cargo', ['llvm-cov', '-p', 'kp-domain', '--locked', '--fail-under-lines', '85']] },
  { id: '5', name: '前端单元/组件测试 + 覆盖率', cmd: ['pnpm', ['test:coverage']] },
  { id: '7', name: '依赖安全审计（pnpm + cargo）', cmd: [null, null], steps: [['pnpm', ['audit', '--audit-level=high']], ['cargo', ['audit', '--file', 'Cargo.lock']]] },
  { id: '8', name: 'Lockfile 一致性', cmd: ['pnpm', ['gate:lockfile']] },
  { id: '9', name: 'Rust 应用构建（tauri build 的本地等价）', cmd: ['cargo', ['build', '--manifest-path', 'src-tauri/Cargo.toml', '--locked']] },
  { id: '10', name: '性能基准（P50 回退 ≤ 20%）', cmd: ['pnpm', ['test:perf']] },
  { id: '11', name: '安全测试集（AC-SEC）', cmd: ['pnpm', ['test:security']] },
  { id: '12', name: '可靠性测试集（AC-REL）', cmd: ['pnpm', ['test:reliability']] },
  { id: '13', name: '命名一致性与文件长度（门禁 13 + CODE-11）', cmd: [null, null], steps: [['pnpm', ['gate:naming']], ['node', ['scripts/check-file-length.mjs', '--self-test']], ['node', ['scripts/check-file-length.mjs']]] },
  { id: '16', name: '路径封装检查（SEC PATH-02）', cmd: ['pnpm', ['gate:path-encapsulation']] },
  { id: '17', name: 'IPC 契约一致性（TS↔Rust）', cmd: [null, null], steps: [['node', ['scripts/verify-ipc-contract.mjs', '--self-test']], ['node', ['scripts/verify-ipc-contract.mjs']]] },
];

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : null;
const skip = (argv.includes('--skip') ? argv[argv.indexOf('--skip') + 1] : '').split(',').filter(Boolean);

function runStep(cmd, args) {
  const printable = [cmd, ...args].join(' ');
  console.log('\n\u25b6 ' + printable);
  const started = Date.now();
  // Windows 下 pnpm/cargo/node 多为 .cmd/.bat，必须经 shell 执行；但 Node 的
  // DEP0190 警告禁止「shell: true + args 数组」组合（参数不转义、可被注入），
  // 因此改为自行转义后传单条命令字符串，POSIX 仍走无 shell 的直连方式。
  const res =
    process.platform === 'win32'
      ? spawnSync([cmd, ...args].map(shellQuote).join(' '), { stdio: 'inherit', shell: true })
      : spawnSync(cmd, args, { stdio: 'inherit' });
  return { code: res.status ?? 1, ms: Date.now() - started, printable };
}

const results = [];
for (const gate of GATES) {
  if (only && gate.id !== only) continue;
  if (skip.includes(gate.id)) { results.push({ id: gate.id, name: gate.name, ok: true, skipped: true, ms: 0 }); continue; }
  console.log('\n' + '='.repeat(72) + '\n门禁 ' + gate.id + '：' + gate.name + '\n' + '='.repeat(72));
  const steps = gate.steps ?? [gate.cmd];
  let ok = true;
  let ms = 0;
  for (const [cmd, args] of steps) {
    const r = runStep(cmd, args);
    ms += r.ms;
    if (r.code !== 0) { console.error('\u274c 失败：' + r.printable + ' (exit ' + r.code + ')'); ok = false; }
  }
  results.push({ id: gate.id, name: gate.name, ok, ms });
}

console.log('\n' + '='.repeat(72) + '\n门禁汇总\n' + '='.repeat(72));
for (const r of results) {
  const mark = r.skipped ? '\u2796 跳过' : r.ok ? '\u2705 通过' : '\u274c 失败';
  console.log('门禁 ' + r.id.padStart(2) + '  ' + mark + '  ' + (r.ms / 1000).toFixed(1).padStart(6) + 's  ' + r.name);
}
const failed = results.filter((r) => !r.ok);
writeFileSync('gate-report.json', JSON.stringify({ generatedAt: new Date().toISOString(), results }, null, 2) + '\n', 'utf8');
console.log('\n报告已写入 gate-report.json');
if (failed.length) { console.error('\n共 ' + failed.length + ' 项门禁失败'); process.exit(1); }
console.log('\n\u2705 全部门禁通过');
