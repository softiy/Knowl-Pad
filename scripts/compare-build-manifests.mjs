#!/usr/bin/env node
// 门禁 9 扩展：比对多次构建的产物清单一致（Vite 8 writeBundle 非确定性回归的规避手段）。
import { readdir } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';

const dirs = process.argv.slice(2).filter((a) => !a.startsWith('--'));
if (dirs.length < 2) { console.error('用法：node scripts/compare-build-manifests.mjs dist-1 dist-2 [dist-3]'); process.exit(1); }

async function manifest(dir) {
  try { return (await readdir(join(dir, 'assets'))).sort().join('|'); }
  catch { return null; }
}
const first = await manifest(dirs[0]);
if (first === null) { console.error('❌ 找不到 ' + join(dirs[0], 'assets')); process.exit(1); }
let ok = true;
for (const dir of dirs.slice(1)) {
  const m = await manifest(dir);
  if (m !== first) { console.error('❌ 产物清单不一致：' + dirs[0] + ' vs ' + dir); ok = false; }
}
if (!ok) process.exit(1);
console.log('✅ ' + dirs.length + ' 次构建产物清单完全一致：' + first);
