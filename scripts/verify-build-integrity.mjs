#!/usr/bin/env node
// 门禁 9 扩展：校验 dist/ 中被 index.html 引用的 chunk 全部存在，且无零字节产物。
import { readFile, readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';

const dist = process.argv.includes('--dist') ? process.argv[process.argv.indexOf('--dist') + 1] : 'dist';
const html = await readFile(join(dist, 'index.html'), 'utf8');
const refs = [...html.matchAll(/(?:src|href)="\/?(assets\/[^"]+)"/g)].map((m) => m[1]);
if (refs.length === 0) { console.error('❌ index.html 未引用任何 assets/ 产物，构建可能失败'); process.exit(1); }

const files = await readdir(join(dist, 'assets'));
const failures = [];
for (const ref of refs) {
  const name = ref.replace('assets/', '');
  if (!files.includes(name)) failures.push('index.html 引用的 chunk 缺失：' + ref);
}
for (const f of files) {
  const size = (await stat(join(dist, 'assets', f))).size;
  if (size === 0) failures.push('零字节产物：' + f);
}
if (failures.length) { console.error('❌ 构建完整性校验失败：'); for (const f of failures) console.error('   - ' + f); process.exit(1); }
console.log('✅ 构建完整性通过：' + refs.length + ' 个引用全部存在，' + files.length + ' 个 chunk 无零字节');
