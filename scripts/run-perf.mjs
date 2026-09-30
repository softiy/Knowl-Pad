#!/usr/bin/env node
// 门禁 10：性能基准（M0 为渲染管线 smoke；PRD §6.1.2 的 16 项指标在 M2+ 逐步补齐）。
// 口径：P50 相对基线回退 > 20% 即失败（PERF-08）。
import { readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import MarkdownIt from 'markdown-it';
import process from 'node:process';

const argv = process.argv.slice(2);
const bi = argv.indexOf('--baseline');
const BASELINE = bi >= 0 && argv[bi + 1] ? argv[bi + 1] : '.perf-baseline.json';
const MAX_REGRESSION = 0.2;
const md = new MarkdownIt({ html: false, linkify: false, typographer: false });

// 构造 ~200KB 的确定性文档
let doc = '';
for (let i = 0; i < 400; i++) {
  doc += '## 标题 ' + i + '\n\n这是一段用于基准测试的中文正文，包含 [[链接' + i + ']] 与 #标签/子 内容。\n\n';
  doc += '- 列表项 A\n- 列表项 B\n\n```ts\nconst x' + i + ' = ' + i + ';\n```\n\n';
}
const bytes = Buffer.byteLength(doc, 'utf8');

function p50(samples) {
  const s = [...samples].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}
function median(samples) {
  const s = [...samples].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

/**
 * 稳健估计：取若干批次「中位数」的最小值。
 * 原因：机器噪声（杀毒扫描、其他进程）只会让单次测量偏慢，
 * 因此最小值最接近真实开销，避免共享/繁忙机器上的假回退。
 */
function measure(batches, perBatch) {
  const medians = [];
  for (let b = 0; b < batches; b++) {
    const samples = [];
    for (let i = 0; i < perBatch; i++) {
      const t0 = performance.now();
      md.render(doc);
      samples.push(performance.now() - t0);
    }
    medians.push(median(samples));
  }
  return Math.min(...medians);
}

measure(2, 5); // 预热，丢弃
const current = Number(measure(7, 20).toFixed(3));
console.log('渲染基准：文档 ' + (bytes / 1024).toFixed(0) + ' KB，min(批次中位数) = ' + current + ' ms');

if (!existsSync(BASELINE) || process.argv.includes('--update-baseline')) {
  await writeFile(BASELINE, JSON.stringify({ metric: 'markdownRenderMinBatchMedianMs', docBytes: bytes, value: current, updatedAt: new Date().toISOString() }, null, 2) + '\n', 'utf8');
  console.log(existsSync(BASELINE) ? '✓ 基线已写入 ' + BASELINE : '✓ 基线已更新');
  process.exit(0);
}
const baseline = JSON.parse(await readFile(BASELINE, 'utf8'));
const ratio = (current - baseline.value) / baseline.value;
if (ratio > MAX_REGRESSION) { console.error('❌ 性能回退 ' + (ratio * 100).toFixed(1) + '%（阈值 20%）'); process.exit(1); }
console.log('✅ 性能无显著回退（' + (ratio * 100).toFixed(1) + '%）');
