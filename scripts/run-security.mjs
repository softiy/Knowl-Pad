#!/usr/bin/env node
// 门禁 11：安全测试集（AC-SEC-01 内容渲染 + AC-SEC-02 Capabilities/CSP 审计）。
import { spawnSync } from 'node:child_process';
import process from 'node:process';

function run(cmd, args) {
  // DEP0190：shell:true 与 args 数组同时使用会被 Node 警告（参数不做转义、只做拼接）。
  // 这里改为把命令拼成**单条字符串**再交给 shell —— 跨平台一致，也不再触发该告警。
  const line = [cmd, ...args]
    .map((a) => (a.includes(' ') ? '"' + a + '"' : a))
    .join(' ');
  console.log('\n▶ ' + line);
  const res = spawnSync(line, { stdio: 'inherit', shell: true });
  return res.status ?? 1;
}
let failed = 0;
// DEBT-21：此前用 `npx vitest` —— 本地没有该依赖时 npx 会**联网自解**（离线/受限环境直接失败或引入不可控版本）。
// 改为直接调用本地安装的 vitest 入口，与 package.json 的 test 脚本保持同一条路径。
failed |= run('node', ['node_modules/vitest/vitest.mjs', 'run', 'tests/security']);
failed |= run('node', ['tests/security/caps-audit.mjs']);
failed |= run('node', ['tests/security/log-privacy.mjs', '--self-test']);
failed |= run('node', ['tests/security/log-privacy.mjs']);
if (failed) { console.error('\n❌ 安全测试集失败'); process.exit(1); }
console.log('\n✅ 安全测试集全部通过');
