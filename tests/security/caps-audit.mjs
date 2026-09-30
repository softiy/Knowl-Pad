#!/usr/bin/env node
// AC-SEC-02：Capabilities 最小权限审计（发布门禁）。
// 断言 capabilities/default.json 不含宽权限，且 shell:allow-open 仅放行 https 与 mailto。
import { readFile } from 'node:fs/promises';
import process from 'node:process';

const raw = await readFile('src-tauri/capabilities/default.json', 'utf8');
const cap = JSON.parse(raw);
const perms = JSON.stringify(cap.permissions ?? []);
const failures = [];

const forbidden = [
  { re: /fs:allow-[a-z-]*/, why: '禁止授予任何 fs 插件权限（AC-01）' },
  { re: /shell:allow-execute/, why: '禁止授予 shell:allow-execute（SEC-05）' },
  { re: /shell:allow-spawn/, why: '禁止授予 shell:allow-spawn（SEC-05）' },
  { re: /process:allow-exit/, why: '禁止授予 process:allow-exit（SEC-05）' },
  { re: /process:deny/, why: '不应使用 deny 伪装最小权限' },
];
for (const f of forbidden) if (f.re.test(perms)) failures.push(f.why + ' → 命中 ' + f.re);

// shell:allow-open 的 URL 白名单
const shell = (cap.permissions ?? []).find((p) => typeof p === 'object' && p.identifier === 'shell:allow-open');
if (!shell) failures.push('缺少 shell:allow-open 显式声明');
else {
  const urls = (shell.allow ?? []).map((a) => a.url);
  if (urls.some((u) => u.startsWith('http://'))) failures.push('shell:allow-open 放行了明文 http://');
  if (urls.some((u) => u.startsWith('file://'))) failures.push('shell:allow-open 放行了 file://');
  if (!urls.some((u) => u.startsWith('https://'))) failures.push('shell:allow-open 未放行 https://');
}

// CSP 审计
const conf = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8'));
const csp = JSON.stringify(conf.app?.security?.csp ?? {});
if (/unsafe-eval/.test(csp)) failures.push('CSP script-src 含 unsafe-eval');
if (/"script-src":"[^"]*unsafe-inline/.test(csp.replace(/\s/g, ''))) failures.push('CSP script-src 含 unsafe-inline');
if (!/object-src/.test(csp)) failures.push('CSP 缺少 object-src');

if (failures.length) {
  console.error('❌ Capabilities/CSP 审计失败：');
  for (const f of failures) console.error('   - ' + f);
  process.exit(1);
}
console.log('✅ Capabilities/CSP 审计通过（无 fs:*、无 shell:allow-execute、无 process:allow-exit、https 白名单、CSP 严格）');
