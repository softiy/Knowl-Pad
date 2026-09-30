#!/usr/bin/env node
// 门禁 11：安全测试集（AC-SEC-01 内容渲染 + AC-SEC-02 Capabilities/CSP 审计）。
import { spawnSync } from 'node:child_process';
import process from 'node:process';

function run(cmd, args) {
  console.log('\n▶ ' + cmd + ' ' + args.join(' '));
  const res = spawnSync(cmd, args, { stdio: 'inherit', shell: process.platform === 'win32' });
  return res.status ?? 1;
}
let failed = 0;
failed |= run('npx', ['vitest', 'run', 'tests/security']);
failed |= run('node', ['tests/security/caps-audit.mjs']);
if (failed) { console.error('\n❌ 安全测试集失败'); process.exit(1); }
console.log('\n✅ 安全测试集全部通过');
