#!/usr/bin/env node
// 基准数据集生成器（PRD §6.1.1）：确定性伪随机（固定 seed）。
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';

const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const tier = arg('--tier', 'standard');
const out = arg('--out', join(process.env.TEMP ?? '/tmp', 'kp-vault'));
const PRESET = { standard: { count: 3000, kb: 2 }, large: { count: 20000, kb: 4 }, stress: { count: 100000, kb: 2 } };
const { count, kb } = PRESET[tier] ?? PRESET.standard;
let seed = 42;
const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff; };

console.log('生成 ' + tier + ' 夹具：' + count + ' 篇 × ~' + kb + 'KB → ' + out);
await mkdir(out, { recursive: true });
for (let i = 0; i < count; i++) {
  const dir = join(out, 'd' + (i % 50));
  await mkdir(dir, { recursive: true });
  const lines = ['---', 'tags: [a, b/c]', 'aliases: [x' + i + ']', '---', '', '# 笔记 ' + i, ''];
  while (Buffer.byteLength(lines.join('\n'), 'utf8') < kb * 1024) {
    lines.push('内容 ' + Math.floor(rnd() * 1e6) + ' 与 [[笔记 ' + Math.floor(rnd() * count) + ']] 以及 #标签/' + (i % 20));
  }
  await writeFile(join(dir, 'note-' + i + '.md'), lines.join('\n'), 'utf8');
  if (i % 5000 === 0 && i > 0) console.log('  ' + i + '/' + count);
}
console.log('✅ 夹具生成完成');
