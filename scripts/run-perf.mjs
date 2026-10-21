#!/usr/bin/env node
// 门禁 10：性能基准（PRD §6.1.2；DEBT-31 后**走生产渲染管线**）。
//
// 口径（2026-10-05 修订 + 2026-10-21 DEBT-31 再修订）：
// ① 每项指标有**绝对预算**（jsdom 标定的异常保护线）；
// ② 相对基线回退超容差（2×）也失败 —— CI 与本机差异由 2× 容忍吸收，绝对预算单独把关；
// ③ **基线按环境隔离**（v3 的 `envs`）：绝对耗时跨机器不可比，当前环境没有基线就硬失败（exit 2）并给出建立方式，
//    **绝不**回退到别的环境的数字（原因见 `scripts/lib/perf-baseline.mjs` 的模块头）。
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
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { cpus } from 'node:os';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';
import {
  BASELINE_VERSION,
  envBlock,
  envNames,
  parseBaseline,
  resolveEnvName,
  selfTest,
  setEnvMetrics,
} from './lib/perf-baseline.mjs';
import { parseMetricsLines, readMetricsFile, runProbe, tailLines } from './lib/perf-probe.mjs';

const argv = process.argv.slice(2);
const bi = argv.indexOf('--baseline');
const BASELINE = bi >= 0 && argv[bi + 1] ? argv[bi + 1] : '.perf-baseline.json';
/**
 * `--emit-baseline <path>`：**只读**地把"当前环境这一块"写到指定文件（CI 作为 artifact 上传，供人工提回基线）。
 *
 * 语义边界（**必须守住**）：
 * - **绝不**写 `BASELINE`（`.perf-baseline.json`）—— 基线只能由人确认后作为提交写入；
 * - 缺当前环境基线时仍然**判失败（exit 2）**，只是"先把数字量出来放进 artifact"，避免为此放宽门禁；
 * - 与 `--update-baseline` 互斥（同时给会 exit 2）。
 */
const ei = argv.indexOf('--emit-baseline');
const EMIT = ei >= 0 && argv[ei + 1] ? argv[ei + 1] : null;
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
 * 退出码：2 = 配置/口径/环境问题；1 = 测量/判定失败。
 */
function fail(title, details, code = 1) {
  console.log('');
  console.log('❌ 门禁 10 失败：' + title);
  for (const line of details) console.log('   ' + line);
  console.error('❌ 门禁 10 失败：' + title + '（详情见 stdout）');
  process.exit(code);
}

if (argv.includes('--self-test')) {
  process.exit(selfTest() ? 0 : 1);
}

const ENV_NAME = resolveEnvName({ env: process.env, cpuCount: cpus().length });
console.log('基线口径 ' + PIPELINE + ' / 环境 ' + ENV_NAME);

const update = argv.includes('--update-baseline');
if (update && EMIT !== null) {
  fail(
    '--emit-baseline 与 --update-baseline 不能同时使用',
    [
      '--emit-baseline 是只读产出（给 CI 传 artifact）；--update-baseline 会改 .perf-baseline.json',
      '请分两次跑：先 --emit-baseline 拿数字，确认后再在本地用 --update-baseline',
    ],
    2,
  );
}
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

let baseline;
try {
  baseline = existsSync(BASELINE)
    ? parseBaseline(readFileSync(BASELINE, 'utf8'))
    : { version: 3, pipeline: PIPELINE, envs: {} };
} catch (err) {
  fail('基线不可用（' + (err && err.message ? err.message : String(err)) + '）', [
    ...(Array.isArray(err?.detail) ? err.detail : []),
    '文件：' + BASELINE,
  ], 2);
}

const current = envBlock(baseline, ENV_NAME);
/** 引导模式：缺当前环境基线、但要求产出 artifact（CI 首次建立基线）。**判定仍然失败**。 */
let bootstrap = false;
if (!current && !update) {
  const details = [
    '当前环境：' + ENV_NAME,
    '已有环境：' + (envNames(baseline).length > 0 ? envNames(baseline).join(', ') : '（无）'),
    '**绝不**跨环境比较：绝对耗时跨机器不可比（24 逻辑核本机 vs 2 vCPU 共享 runner）',
    '建立方式（在该环境上跑一次）：pnpm test:perf -- --update-baseline',
    'CI 上可用 KP_PERF_ENV 显式指定环境名（工作流已钉死为 ci-ubuntu-2vcpu）',
  ];
  if (EMIT === null) {
    fail('基线里没有当前环境的数值', details, 2);
  }
  console.log('');
  console.log('⚠️ 基线里没有当前环境的数值：本次会测量并产出 artifact，但**判定仍失败（exit 2）**');
  for (const line of details) console.log('   ' + line);
  bootstrap = true;
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
  const value = measured[metric.key];
  const stored = current?.metrics?.[metric.key];
  console.log(
    metric.label + '：min(批次中位数) = ' + value.value.toFixed(1) + ' ms（' + value.docBytes + ' 字节）',
  );
  if (update || bootstrap) continue;
  const budget = metric.budgetMs;
  if (budget && value.value > budget) {
    console.log('  ❌ 超出预算 ' + budget + ' ms（实测 ' + value.value.toFixed(1) + ' ms）');
    failed = true;
    continue;
  }
  if (!stored) {
    console.log('  ❌ 本环境基线中没有该指标（新增指标需在本次提交内用 --update-baseline 建立基线）');
    failed = true;
    continue;
  }
  const ratio = (value.value - stored.value) / stored.value;
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

const now = new Date().toISOString();
const metricsForEnv = Object.fromEntries(METRICS.map((m) => [m.key, { ...measured[m.key], updatedAt: now }]));

if (update) {
  const next = setEnvMetrics(baseline, ENV_NAME, metricsForEnv, now);
  next.pipeline = PIPELINE;
  writeFileSync(BASELINE, JSON.stringify(next, null, 2) + '\n', 'utf8');
  console.log('✓ 已写入 ' + BASELINE + ' 的 envs[' + ENV_NAME + ']（口径 ' + PIPELINE + '，其它环境保持不变）');
  process.exit(0);
}

if (EMIT !== null) {
  // **只读**产出：给 CI 传 artifact，供人下载后作为提交写进 .perf-baseline.json 的 envs。
  // 这里刻意用 `baseline.envs[ENV_NAME]` 的结构（可直接贴），并且**不**碰 BASELINE。
  mkdirSync(dirname(EMIT), { recursive: true });
  const payload = {
    env: ENV_NAME,
    pipeline: PIPELINE,
    generatedAt: now,
    note: '本文件只读产出，不修改 .perf-baseline.json；人工确认后把 baseline.envs 下的这一块贴进仓库基线并提交',
    baseline: { version: BASELINE_VERSION, pipeline: PIPELINE, envs: { [ENV_NAME]: { metrics: metricsForEnv, updatedAt: now } } },
  };
  writeFileSync(EMIT, JSON.stringify(payload, null, 2) + '\n', 'utf8');
  console.log('✓ 已写出 ' + EMIT + '（env=' + ENV_NAME + '；**只读**：未修改 ' + BASELINE + '）');
  if (bootstrap) {
    console.log('  ❌ 本次判定仍失败（exit 2）：本环境还没有基线 —— 下载 artifact、人工提交该块后再跑一次');
    process.exit(2);
  }
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