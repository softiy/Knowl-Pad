#!/usr/bin/env node
// scripts/gen-latest-json.mjs --target <triple> --version <semver>
// 生成 Tauri updater 的 latest.json 片段（每平台一个），供 merge-latest-json.mjs 合并。
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';

const argv = process.argv.slice(2);
const arg = (k) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : undefined; };
const target = arg('--target');
const version = arg('--version');
if (!target || !version) { console.error('用法：node scripts/gen-latest-json.mjs --target <triple> --version <semver>'); process.exit(1); }

const bundleDir = join('src-tauri', 'target', target, 'release', 'bundle');
async function findSig() {
  async function walk(d) {
    let out = [];
    let es = [];
    try { es = await readdir(d, { withFileTypes: true }); } catch { return out; }
    for (const e of es) out = out.concat(e.isDirectory() ? await walk(join(d, e.name)) : (e.name.endsWith('.sig') ? [join(d, e.name)] : []));
    return out;
  }
  return walk(bundleDir);
}
const sigs = await findSig();
const platformKey = target.replace(/-unknown-linux-gnu$/, '-unknown-linux-gnu');
const platforms = {};
for (const s of sigs) {
  // 需与 tauri.conf.json 的 updater endpoint 规则一致：这里只记录签名内容，URL 由发布时填写
  platforms[platformKey] = { signature: (await readFile(s, 'utf8')).trim(), url: 'GITEE_PLACEHOLDER' };
}
const out = { version, notes: '', pub_date: new Date().toISOString(), platforms };
await writeFile('latest.json', JSON.stringify(out, null, 2) + '\n', 'utf8');
console.log('✓ latest.json（' + target + '）：' + Object.keys(platforms).length + ' 个平台条目');
