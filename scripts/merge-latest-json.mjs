#!/usr/bin/env node
// scripts/merge-latest-json.mjs --dir <artifacts> --out latest.json
// 合并各平台 job 产出的 latest.json（各含一个 platforms 键）。
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';

const argv = process.argv.slice(2);
const arg = (k) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : undefined; };
const dir = arg('--dir') || 'artifacts';
const outFile = arg('--out') || 'latest.json';

async function walk(d) {
  let files = [];
  let es = [];
  try { es = await readdir(d, { withFileTypes: true }); } catch { return files; }
  for (const e of es) {
    const p = join(d, e.name);
    if (e.isDirectory()) files = files.concat(await walk(p));
    else if (e.name === 'latest.json') files.push(p);
  }
  return files;
}
const parts = await walk(dir);
if (!parts.length) { console.error('❌ 在 ' + dir + ' 下找不到 latest.json'); process.exit(1); }

const merged = { version: null, notes: '', pub_date: new Date().toISOString(), platforms: {} };
for (const p of parts) {
  const j = JSON.parse(await readFile(p, 'utf8'));
  merged.version = merged.version || j.version;
  Object.assign(merged.platforms, j.platforms || {});
}
if (merged.version === null) { console.error('❌ latest.json 缺少 version'); process.exit(1); }
await writeFile(outFile, JSON.stringify(merged, null, 2) + '\n', 'utf8');
console.log('✓ 合并 ' + parts.length + ' 份 → ' + outFile + '（' + Object.keys(merged.platforms).length + ' 个平台）');
