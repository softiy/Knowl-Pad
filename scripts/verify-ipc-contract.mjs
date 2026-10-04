#!/usr/bin/env node
// 门禁 6 扩展：IPC 契约一致性——TS 侧调用的命令名必须都能在 Rust 侧找到 #[tauri::command]。
import { readFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';

async function walk(dir) {
  let out = [];
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out = out.concat(await walk(p));
    else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}
const rustFiles = await walk('src-tauri/src');
const rustCommands = new Set();
for (const f of rustFiles) {
  const src = await readFile(f, 'utf8');
  for (const m of src.matchAll(/#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) rustCommands.add(m[1]);
}

const ts = await readFile('src/core/ipc/commands.ts', 'utf8');
/** 从 TS 源码抽取被调用的命令名（供主流程与 --self-test 共用）。 */
export function extractTsCommands(source) {
  return new Set(
    [...source.matchAll(/(?:call|callVoid)(?:<[^>]*>)?\(\s*['"]([a-z0-9_]+)['"]/g)].map((m) => m[1]),
  );
}

// 自检：证明解析器能识别单引号、双引号与泛型写法——避免"门禁假阴性"（曾因只认单引号而漏检）
if (process.argv.includes('--self-test')) {
  const cases = [
    ["call('a_b')", ['a_b']],
    ['call("a_b")', ['a_b']],
    ["call<Foo>('a_b')", ['a_b']],
    ["callVoid('x_y', { args: {} })", ['x_y']],
    ['callVoid( "x_y" )', ['x_y']],
  ];
  let pass = 0;
  for (const [src, want] of cases) {
    const got = [...extractTsCommands(src)];
    if (got.length === want.length && got.every((n, i) => n === want[i])) pass += 1;
    else console.error('❌ 自检失败:', src, '期望', want, '实得', got);
  }
  console.log('自检：' + pass + '/' + cases.length + ' 通过');
  process.exit(pass === cases.length ? 0 : 1);
}

const tsCommands = extractTsCommands(ts);

const missing = [...tsCommands].filter((c) => !rustCommands.has(c));
console.log('Rust 命令 ' + rustCommands.size + ' 个：' + [...rustCommands].sort().join(', '));
console.log('TS 调用 ' + tsCommands.size + ' 个：' + [...tsCommands].sort().join(', '));
if (missing.length) { console.error('❌ TS 侧调用了不存在的 Command：' + missing.join(', ')); process.exit(1); }
const unexposed = [...rustCommands].filter((c) => !tsCommands.has(c));
if (unexposed.length) console.warn('⚠️ Rust 已实现但 TS 未封装的 Command（仅提示，不阻断）：' + unexposed.sort().join(', '));
console.log('✅ IPC 契约一致');
