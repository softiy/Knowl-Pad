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
const tsCommands = new Set([...ts.matchAll(/(?:call|callVoid)(?:<[^>]*>)?\('([a-z0-9_]+)'/g)].map((m) => m[1]));

const missing = [...tsCommands].filter((c) => !rustCommands.has(c));
console.log('Rust 命令 ' + rustCommands.size + ' 个：' + [...rustCommands].sort().join(', '));
console.log('TS 调用 ' + tsCommands.size + ' 个：' + [...tsCommands].sort().join(', '));
if (missing.length) { console.error('❌ TS 侧调用了不存在的 Command：' + missing.join(', ')); process.exit(1); }
const unexposed = [...rustCommands].filter((c) => !tsCommands.has(c));
if (unexposed.length) console.warn('⚠️ Rust 已实现但 TS 未封装的 Command（仅提示，不阻断）：' + unexposed.sort().join(', '));
console.log('✅ IPC 契约一致');
