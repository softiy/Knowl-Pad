#!/usr/bin/env node
// 门禁 10：性能基准（PRD §6.1.2；DEBT-31 后**走生产渲染管线**）。
//
// 口径（2026-10-05 修订 + 2026-10-21 DEBT-31 再修订）：
// ① 每项指标有**绝对预算**（jsdom 标定的异常保护线）；
// ② 相对基线回退超容差（2×）也失败 —— CI 与本机差异由 2× 容忍吸收，绝对预算单独把关。
//
// **为什么不再在这里 import markdown-it**：DEBT-31 实测旧路径（自建 `new MarkdownIt({html:false})`）不含
// `renderer.ts` 的 html:true / DOMPurify / 图片策略 / kp-inline / fence 高亮，其"预算内"结论不代表用户路径
// （0.52MB 夹具：旧 35.7ms vs 生产 781ms，约 22×）。测量宿主是 `tests/perf/render-pipeline.perf.spec.ts`
// （vitest + jsdom + 生产 `renderMarkdown`），本脚本只负责**跑它、读指标、判预算/回退**。
//
// **2026-10-21 CI 事故（#160）后的加固**：指标不再靠 stdout 行解析 —— `--reporter=dot` 的进度点不带换行，
// 指标行会被粘成 `·PERF_METRIC …`，行首锚点 `^PERF_METRIC` 于是静默漏掉一条，表现为"large 通过、small 消失、
// 退出码非零且没有解释"。现在探针把指标**写成 JSON 文件**（`KP_PERF_OUT`），脚本读文件；行解析只作兜底，
// 且所有失败分支都会把**可定位的原因**打到 stdout（门禁汇总一定会显示的那一路）。
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import process from 'node:process';
import { loadBaseline, parseMetricsLines, readMetricsFile, runProbe, tailLines } from './lib/perf-probe.mjs';

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
 * 打印**可定位**的失败原因。
 *
 * 刻意打到 **stdout**：`scripts/run-gates.mjs` 会把子进程的 stdout 原样回显，而 CI 日志里 stderr 有时与 stdout
 * 交错甚至被裁剪（#160 事故里"没有任何错误说明"就是这么来的）。另加一行 stderr 摘要，便于单独看错误流。
 * 退出码：2 = 配置/口径问题；1 = 测量/判定失败。
 */
function fail(title, details, code = 1) {
  console.log('');
  console.log('❌ 门禁 10 失败：' + title);
  for (const line of details) console.log('   ' + line);
  console.error('❌ 门禁 10 失败：' + title + '（详情见 stdout）');
  process.exit(code);
}

const baseline = loadBaseline(existsSync(BASELINE) ? readFileSync(BASELINE, 'utf8') : null, PIPELINE);
const update = argv.includes('--update-baseline');
if (!existsSync(BASELINE) && !update) {
  fail(
    '缺少性能基线 ' + BASELINE,
    [
      '首次建立请显式执行：pnpm test:perf -- --update-baseline',
      '（门禁不得自愈：删掉基线文件就能通过，等于把性能门禁变成永真）',
    ],
    2,
  );
}

const probe = runProbe({ root, vitest });
if (probe.exitCode !== 0) {
  fail('测量宿主执行失败（探针进程非零退出）', [
    '退出码：' + String(probe.exitCode) + '，signal：' + String(probe.signal) + '，耗时 ' + probe.elapsedMs + 'ms',
    '指标文件：' + probe.filePath + (probe.fileExists ? '（存在）' : '（不存在）'),
    '探针输出尾部（最近 30 行）：',
    ...tailLines(probe.out, 30).map((l) => '  ' + l),
  ]);
}

const fromFile = readMetricsFile(probe);
const fallback = parseMetricsLines(probe.out);
const measured = fromFile.metrics ?? fallback.metrics;
if (fromFile.error !== null) {
  console.log('⚠️ 指标文件不可用（' + fromFile.error + '），已回退到 stdout 行解析（命中 ' + fallback.hits + ' 条）');
}

const received = Object.keys(measured ?? {});
const missing = METRICS.map((m) => m.key).filter((k) => measured?.[k] === undefined);
if (missing.length > 0) {
  fail('探针没有产出全部指标', [
    '缺失指标：' + missing.join(', '),
    '收到的指标：' + (received.length > 0 ? received.join(', ') : '（无）'),
    '指标文件：' + probe.filePath + (fromFile.error ? '（不可用：' + fromFile.error + '）' : '（已读取）'),
    '行解析兜底命中：' + fallback.hits + ' 条（若指标行被 reporter 的进度点粘住，行首锚点会漏 —— 这正是 #160）',
    '探针输出尾部（最近 20 行）：',
    ...tailLines(probe.out, 20).map((l) => '  ' + l),
  ]);
}

let failed = false;
for (const metric of METRICS) {
  const current = measured[metric.key];
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
  const budget = metric.budgetMs;
  if (budget && current.value > budget) {
    console.log('  ❌ 超出预算 ' + budget + ' ms（实测 ' + current.value.toFixed(1) + ' ms）');
    failed = true;
    continue;
  }
  if (!stored) {
    console.log('  ❌ 基线中没有该指标（新增指标需在本次提交内用 --update-baseline 建立基线）');
    failed = true;
    continue;
  }
  const ratio = (current.value - stored.value) / stored.value;
  if (ratio > MAX_REGRESSION) {
    console.log('  ❌ 性能回退 ' + (ratio * 100).toFixed(1) + '%（容差 ' + MAX_REGRESSION * 100 + '%，基线 ' + stored.value + ' ms）');
    failed = true;
  } else {
    console.log(
      '  ✅ 预算内且无显著回退（' +
        (ratio * 100).toFixed(1) +
        '%，基线 ' +
        stored.value +
        ' ms，预算 ' +
        budget +
        ' ms）',
    );
  }
}

rmSync(probe.dir, { recursive: true, force: true });

if (update) {
  baseline.version = 2;
  baseline.pipeline = PIPELINE;
  writeFileSync(BASELINE, JSON.stringify(baseline, null, 2) + '\n', 'utf8');
  console.log('✓ 基线已写入 ' + BASELINE + '（口径 ' + PIPELINE + '）');
  process.exit(0);
}
if (baseline.pipeline !== PIPELINE) {
  fail(
    '基线的测量口径与当前不一致',
    [
      '基线 pipeline=' + String(baseline.pipeline) + '，当前=' + PIPELINE,
      '旧口径是与生产管线不可比的数字，必须用 --update-baseline 重建。',
    ],
    2,
  );
}
process.exit(failed ? 1 : 0);