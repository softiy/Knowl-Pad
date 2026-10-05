#!/usr/bin/env node
// 门禁 10：性能基准（PRD §6.1.2 的指标随里程碑逐步补齐）。
// 口径（2026-10-05 修订）：① 每项指标有**绝对预算**（机器无关，硬性）；② 相对基线回退超容差（2×）也失败。
// 修订原因：基线是在某台机器上测的，CI runner 与本机实测差 50%+，原 20% 阈值会产生跨机假失败。
import { readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import MarkdownIt from 'markdown-it';
import process from 'node:process';

const argv = process.argv.slice(2);
const bi = argv.indexOf('--baseline');
const BASELINE = bi >= 0 && argv[bi + 1] ? argv[bi + 1] : '.perf-baseline.json';
// 跨机器容差：本机与 CI runner 实测差异可达 55%（小文档 +47.7%、大文档 +55.6%），
// 因此回归判定放宽到 2×（仍能抓住 O(n²) 这类算法级回退），绝对预算单独硬性把关。
const MAX_REGRESSION = 1.0;
const md = new MarkdownIt({ html: false, linkify: false, typographer: false });

/** 构造确定性的 Markdown 文档。 */
function buildDoc(sections, para) {
  let doc = '';
  for (let i = 0; i < sections; i++) {
    doc += '## 标题 ' + i + '\n\n' + para.replace(/\{i\}/g, String(i)) + '\n\n';
    doc += '- 列表项 A\n- 列表项 B\n\n```ts\nconst x' + i + ' = ' + i + ';\n```\n\n';
  }
  return doc;
}

function p50orMedian(samples) {
  const s = [...samples].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

/**
 * 稳健估计：取若干批次「中位数」的最小值。
 * 机器噪声（杀毒扫描、其他进程）只会让单次测量偏慢，最小值最接近真实开销。
 */
function measure(doc, batches, perBatch) {
  const medians = [];
  for (let b = 0; b < batches; b++) {
    const samples = [];
    for (let i = 0; i < perBatch; i++) {
      const t0 = performance.now();
      md.render(doc);
      samples.push(performance.now() - t0);
    }
    medians.push(p50orMedian(samples));
  }
  return Math.min(...medians);
}

const smallDoc = buildDoc(400, '这是一段用于基准测试的中文正文，包含 [[链接{i}]] 与 #标签/子 内容。');
// AC-EDITOR-05 的输入规模：约 2MB 的 Markdown（阅读/分屏渲染都要走这条解析路径）
const largeDoc = buildDoc(12000, '这是一段用于大文件基准测试的中文正文，包含 [[链接{i}]] 与 #标签/子 内容，用于逼近 2MB 量级。');

const METRICS = [
  {
    key: 'markdownRenderSmall',
    // 预算为「异常保护」：正常约 4–7ms，超过 200ms 说明实现出了大问题
    budgetMs: 200,
    label: '小文档渲染（约 ' + (Buffer.byteLength(smallDoc, 'utf8') / 1024).toFixed(0) + ' KB）',
    docBytes: Buffer.byteLength(smallDoc, 'utf8'),
    run: () => Number(measure(smallDoc, 7, 20).toFixed(3)),
  },
  {
    key: 'markdownRenderLarge',
    // AC-EDITOR-05：2MB 级文档「打开 <2s」——解析+渲染是其中我们这一侧的部分
    budgetMs: 2000,
    label: '大文档渲染（约 ' + (Buffer.byteLength(largeDoc, 'utf8') / 1024 / 1024).toFixed(1) + ' MB，AC-EDITOR-05 规模）',
    docBytes: Buffer.byteLength(largeDoc, 'utf8'),
    run: () => Number(measure(largeDoc, 3, 3).toFixed(3)),
  },
];

function loadBaseline(raw) {
  if (!raw) return { version: 2, metrics: {} };
  const parsed = JSON.parse(raw);
  if (parsed.version === 2 && parsed.metrics) return parsed;
  // 旧格式（单指标）自动迁移，避免基线丢失
  return { version: 2, metrics: { markdownRenderSmall: { value: parsed.value, docBytes: parsed.docBytes, updatedAt: parsed.updatedAt } } };
}

const baseline = loadBaseline(existsSync(BASELINE) ? await readFile(BASELINE, 'utf8') : null);
const update = !existsSync(BASELINE) || argv.includes('--update-baseline');
let failed = false;

// 预热（丢弃）
measure(smallDoc, 2, 5);

for (const metric of METRICS) {
  const current = metric.run();
  const stored = baseline.metrics[metric.key];
  console.log(metric.label + '：min(批次中位数) = ' + current + ' ms');
  if (update || !stored) {
    baseline.metrics[metric.key] = { value: current, docBytes: metric.docBytes, updatedAt: new Date().toISOString() };
    console.log('  ✓ 基线已' + (stored ? '更新' : '写入'));
    continue;
  }
  if (metric.budgetMs && current > metric.budgetMs) {
    console.error('  ❌ 超出预算 ' + metric.budgetMs + ' ms（实测 ' + current + ' ms）');
    failed = true;
    continue;
  }
  const ratio = (current - stored.value) / stored.value;
  if (ratio > MAX_REGRESSION) {
    console.error('  ❌ 性能回退 ' + (ratio * 100).toFixed(1) + '%（容差 ' + MAX_REGRESSION * 100 + '%，基线 ' + stored.value + ' ms）');
    failed = true;
  } else {
    console.log(
      '  ✅ 预算内且无显著回退（' + (ratio * 100).toFixed(1) + '%，基线 ' + stored.value + ' ms，预算 ' + metric.budgetMs + ' ms）',
    );
  }
}

if (update) {
  baseline.version = 2;
  await writeFile(BASELINE, JSON.stringify(baseline, null, 2) + '\n', 'utf8');
  console.log('✓ 基线已写入 ' + BASELINE);
  process.exit(0);
}
process.exit(failed ? 1 : 0);
