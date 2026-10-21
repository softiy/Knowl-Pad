// 门禁 10 的探针辅助（从 `scripts/run-perf.mjs` 拆出；CODE-11 的文件长度纪律）。
//
// 这里只做"跑测量宿主 + 读指标 + 兜底解析"，判定与阈值全部留在 `run-perf.mjs`（单一职责，便于评审）。
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** 取文本最后 N 行（诊断用；避免把整份 vitest 输出刷进日志）。 */
export function tailLines(text, count) {
  const all = String(text ?? '').trimEnd().split(/\r?\n/);
  return all.slice(Math.max(0, all.length - count));
}

/**
 * 跑测量宿主（vitest + jsdom + 生产管线）。
 *
 * - `KP_PERF_OUT` 指向本次运行**独有**的临时 JSON（脚本读它判定，见 `run-perf.mjs` 文件头的事故说明）；
 * - `--max-old-space-size=4096`：jsdom 里 0.52MB 文档反复渲染的实测需要（默认堆会 OOM）。
 */
export function runProbe({ root, vitest }) {
  const dir = mkdtempSync(join(tmpdir(), 'kp-perf-'));
  const filePath = join(dir, 'metrics.json');
  const env = { ...process.env, KP_PERF_OUT: filePath };
  if (env.KP_PERF_LEGACY === undefined) env.KP_PERF_LEGACY = '0';
  const started = Date.now();
  const child = spawnSync(
    process.execPath,
    ['--max-old-space-size=4096', vitest, 'run', '--config', 'vitest.perf.config.ts', '--reporter=dot'],
    { cwd: root, encoding: 'utf8', env, shell: false },
  );
  const out = (child.stdout ?? '') + (child.stderr ?? '');
  return {
    out,
    filePath,
    dir,
    exitCode: child.status,
    signal: child.signal,
    elapsedMs: Date.now() - started,
    fileExists: existsSync(filePath),
  };
}

/** 读探针写下的指标 JSON；失败时返回 `{ metrics: null, error }`（不抛，交给诊断块说明）。 */
export function readMetricsFile(probe) {
  if (!probe.fileExists) return { metrics: null, error: '文件不存在（探针可能没跑到写文件那一步）' };
  try {
    const parsed = JSON.parse(readFileSync(probe.filePath, 'utf8'));
    if (!parsed || typeof parsed.metrics !== 'object' || parsed.metrics === null) {
      return { metrics: null, error: '文件里没有 metrics 对象' };
    }
    return { metrics: parsed.metrics, error: null };
  } catch (err) {
    return { metrics: null, error: String(err && err.message ? err.message : err) };
  }
}

/**
 * 兜底：从 stdout 里捞指标行。**故意不锚定行首** —— 点号/ANSI 可能粘在前面（#160 事故的根因）。
 * 命中数写进诊断块，方便判断"文件缺失时兜底是否也失效"。
 */
export function parseMetricsLines(out) {
  const metrics = {};
  let hits = 0;
  for (const raw of String(out ?? '').split(/\r?\n/)) {
    const m = /PERF_METRIC (\S+) ([\d.]+) (\d+)/.exec(raw);
    if (!m) continue;
    if (raw.includes('LEGACY_PERF_METRIC') || raw.includes('FULL_PERF_METRIC')) continue;
    hits += 1;
    metrics[m[1]] = { value: Number(m[2]), docBytes: Number(m[3]) };
  }
  return { metrics, hits };
}

/** 读基线（兼容旧格式：单指标自动迁移）。 */
export function loadBaseline(raw, pipeline) {
  if (!raw) return { version: 2, pipeline, metrics: {} };
  const parsed = JSON.parse(raw);
  if (parsed.version === 2 && parsed.metrics) return parsed;
  return {
    version: 2,
    pipeline,
    metrics: { markdownRenderSmall: { value: parsed.value, docBytes: parsed.docBytes, updatedAt: parsed.updatedAt } },
  };
}