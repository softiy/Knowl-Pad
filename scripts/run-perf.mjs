#!/usr/bin/env node
// 门禁 10：性能基准（PRD §6.1.2；DEBT-31 后**走生产渲染管线**）。
//
// 口径（2026-10-05 修订 + 2026-10-21 DEBT-31 再修订）：
// ① 每项指标有**绝对预算**（jsdom 标定的异常保护线）；
// ② 相对基线回退超容差（2×）也失败 —— CI 与本机差异由 2× 容忍吸收，绝对预算单独把关。
//
// **为什么不再在这里 import markdown-it**：DEBT-31 实测旧路径（自建 `new MarkdownIt({html:false})`）不含
// `renderer.ts` 的 html:true / DOMPurify / 图片策略 / kp-inline / fence 高亮，其"预算内"结论不代表用户路径
// （0.52MB 夹具：旧 35.7ms vs 生产 781ms，约 22×）。现在的测量宿主是 `tests/perf/render-pipeline.perf.spec.ts`
// （vitest + jsdom + 生产 `renderMarkdown`），本脚本只负责**跑它、解析数字、判预算/回退**。
import { spawnSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const argv = process.argv.slice(2);
const bi = argv.indexOf('--baseline');
const BASELINE = bi >= 0 && argv[bi + 1] ? argv[bi + 1] : '.perf-baseline.json';
// 跨机器容差：本机与 CI runner 实测差异可达 55%，因此回归判定放宽到 2×（仍能抓住 O(n²) / 多一遍管线这类
// 算法级回退），绝对预算单独硬性把关。**不得再放宽**：负对照见 DEBT-31 的处置记录。
const MAX_REGRESSION = 1.0;
/** 基线口径标记：基线与测量宿主必须一致，否则数字不可比（DEBT-31）。 */
const PIPELINE = 'production';

/** 指标表：预算按 jsdom 标定的基线推导（见注释），不是拍脑袋。 */
const METRICS = [
  {
    key: 'markdownRenderSmall',
    // 实测生产管线（jsdom）约 125ms（67KB）⇒ 预算取 3× ≈ 375，向上取整到 400：异常保护线，
    // 真正的回退判定靠下面的 2× 规则（更紧）。
    budgetMs: 400,
    label: '小文档渲染（67KB，生产管线）',
  },
  {
    key: 'markdownRenderLarge',
    // 夹具 0.52MB（与 DEBT-31 探针同规模）：实测生产管线约 800–840ms ⇒ 预算取 3× 并向上取整到 2600。
    // AC-EDITOR-05 的 2MB「<2s」是**真机**发布前判定项（技术方案 §11.6），不在本门禁里用 jsdom 数字冒充。
    budgetMs: 2600,
    label: '门禁夹具渲染（0.52MB，生产管线）',
  },
];

const root = fileURLToPath(new URL('..', import.meta.url));
const vitest = fileURLToPath(new URL('../node_modules/vitest/vitest.mjs', import.meta.url));

/**
 * 跑测量宿主并解析 `PERF_METRIC <key> <ms> <docBytes>`。
 *
 * 用子进程而不是直接 import：生产管线是 TS + 路径别名 + 依赖 DOM 的 DOMPurify，必须由 vitest（jsdom）跑；
 * `--max-old-space-size=4096` 是 jsdom 里 0.52MB 文档反复渲染的实测需要（默认堆会 OOM）。
 */
function collectMetrics() {
  const env = { ...process.env };
  if (process.env.KP_PERF_LEGACY === undefined) env.KP_PERF_LEGACY = '0';
  const child = spawnSync(
    process.execPath,
    ['--max-old-space-size=4096', vitest, 'run', '--config', 'vitest.perf.config.ts', '--reporter=dot'],
    { cwd: root, encoding: 'utf8', env, shell: false },
  );
  const out = (child.stdout ?? '') + (child.stderr ?? '');
  if (child.status !== 0) {
    console.error('❌ 性能探针执行失败（vitest 退出码 ' + String(child.status) + '）');
    console.error(out.trim());
    process.exit(1);
  }
  const metrics = {};
  for (const line of out.split(/\r?\n/)) {
    const m = /^PERF_METRIC (\S+) ([\d.]+) (\d+)$/.exec(line.trim());
    if (m) metrics[m[1]] = { value: Number(m[2]), docBytes: Number(m[3]) };
  }
  return metrics;
}

function loadBaseline(raw) {
  if (!raw) return { version: 2, pipeline: PIPELINE, metrics: {} };
  const parsed = JSON.parse(raw);
  if (parsed.version === 2 && parsed.metrics) return parsed;
  // 旧格式（单指标）自动迁移，避免基线丢失
  return {
    version: 2,
    pipeline: PIPELINE,
    metrics: { markdownRenderSmall: { value: parsed.value, docBytes: parsed.docBytes, updatedAt: parsed.updatedAt } },
  };
}

const baseline = loadBaseline(existsSync(BASELINE) ? await readFile(BASELINE, 'utf8') : null);
const update = argv.includes('--update-baseline');
if (!existsSync(BASELINE) && !update) {
  // 门禁不得"自愈"：基线缺失时写一份新的并放行，等于把性能门禁变成永真（删文件即可绕过）。
  console.error('❌ 缺少性能基线 ' + BASELINE + '。首次建立请显式执行：pnpm test:perf -- --update-baseline');
  process.exit(2);
}

const measured = collectMetrics();
let failed = false;

for (const metric of METRICS) {
  const current = measured[metric.key];
  if (!current) {
    console.error('❌ 探针没有输出指标 ' + metric.key + '（测量宿主坏了？）');
    failed = true;
    continue;
  }
  const stored = baseline.metrics[metric.key];
  console.log(
    metric.label + '：min(批次中位数) = ' + current.value.toFixed(1) + ' ms（' + current.docBytes + ' 字节）',
  );
  if (update) {
    baseline.metrics[metric.key] = {
      value: current.value,
      docBytes: current.docBytes,
      updatedAt: new Date().toISOString(),
    };
    console.log('  ✓ 基线已' + (stored ? '更新' : '写入'));
    continue;
  }
  if (metric.budgetMs && current.value > metric.budgetMs) {
    console.error('  ❌ 超出预算 ' + metric.budgetMs + ' ms（实测 ' + current.value.toFixed(1) + ' ms）');
    failed = true;
    continue;
  }
  if (!stored) {
    console.error('  ❌ 基线中没有该指标（新增指标需在本次提交内用 --update-baseline 建立基线）');
    failed = true;
    continue;
  }
  const ratio = (current.value - stored.value) / stored.value;
  if (ratio > MAX_REGRESSION) {
    console.error(
      '  ❌ 性能回退 ' +
        (ratio * 100).toFixed(1) +
        '%（容差 ' +
        MAX_REGRESSION * 100 +
        '%，基线 ' +
        stored.value +
        ' ms）',
    );
    failed = true;
  } else {
    console.log(
      '  ✅ 预算内且无显著回退（' +
        (ratio * 100).toFixed(1) +
        '%，基线 ' +
        stored.value +
        ' ms，预算 ' +
        metric.budgetMs +
        ' ms）',
    );
  }
}

if (update) {
  baseline.version = 2;
  baseline.pipeline = PIPELINE;
  await writeFile(BASELINE, JSON.stringify(baseline, null, 2) + '\n', 'utf8');
  console.log('✓ 基线已写入 ' + BASELINE + '（口径 ' + PIPELINE + '）');
  process.exit(0);
}
if (baseline.pipeline !== PIPELINE) {
  console.error('❌ 基线的测量口径与当前不一致（基线=' + String(baseline.pipeline) + '，当前=' + PIPELINE + '）：');
  console.error('   旧口径是与生产管线不可比的数字，必须用 --update-baseline 重建。');
  process.exit(2);
}
process.exit(failed ? 1 : 0);