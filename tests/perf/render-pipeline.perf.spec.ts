// @vitest-environment jsdom
/**
 * **门禁 10 的测量宿主**（DEBT-31 处置）：在真实 DOM 环境里走**生产渲染管线**。
 *
 * 为什么不能在 `scripts/run-perf.mjs` 里自建 `new MarkdownIt({html:false})`：那条路径与 `renderer.ts` 差着
 * `html:true`、DOMPurify 净化、图片策略、自研 kp-link/kp-tag inline 规则、fence 高亮 —— DEBT-31 实测生产约为
 * 旧指标的 9.8×（0.52MB：65ms vs 634ms），旧数字"预算内"不代表用户路径。
 *
 * 为什么用 vitest 当宿主：生产管线是 TS + 路径别名（`@core/*`）+ 依赖 DOM 的 DOMPurify，裸 node 跑不了；
 * vitest + jsdom 是仓库里既有的、与真实浏览器最接近的执行环境（`tests/perf/**` 已被主配置 exclude，不进日常单测）。
 *
 * **夹具规模的取舍（2026-10-21 实测）**：2.5MB（AC-EDITOR-05 规模）在 jsdom 里走完整管线的单次耗时约 4s、
 * 峰值内存需要 ~8GB 堆才不 OOM —— 作为每次 CI 都跑的门禁既慢又可能撑爆 runner。因此：
 * - 门禁夹具取 **0.52MB**（与 DEBT-31 探针同规模，足以覆盖"解析 + 净化 + 高亮"的整条链路）；
 * - **2.5MB 全量**作为可复现证据：`KP_PERF_FULL=1 node … --max-old-space-size=8192` 时额外输出 `FULL_PERF_METRIC`，
 *   不进预算；AC-EDITOR-05 的 2MB 绝对耗时仍按既有口径在**发布前、真机**上人工判定（技术方案 §11.6 / 附录 B.5）。
 *
 * 输出协议（`scripts/run-perf.mjs` 解析）：`PERF_METRIC <key> <ms> <docBytes>`；
 * 旧口径对照只在 `KP_PERF_LEGACY=1` 时输出（`LEGACY_PERF_METRIC`），全量对照在 `KP_PERF_FULL=1` 时输出（`FULL_PERF_METRIC`），
 * 两者都不参与门禁。
 */
import { describe, expect, it } from 'vitest';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import MarkdownIt from 'markdown-it';
import { clearRenderCache, renderMarkdown } from '@core/markdown/renderer';

/** 与旧脚本逐字一致的确定性文档生成器（保证新旧数字可比）。 */
function buildDoc(sections: number, para: string): string {
  let doc = '';
  for (let i = 0; i < sections; i += 1) {
    doc += '## 标题 ' + i + '\n\n' + para.replace(/\{i\}/g, String(i)) + '\n\n';
    doc += '- 列表项 A\n- 列表项 B\n\n```ts\nconst x' + i + ' = ' + i + ';\n```\n\n';
  }
  return doc;
}

function median(samples: number[]): number {
  const sorted = [...samples].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

/** 稳健估计：取若干批次「中位数」的最小值（机器噪声只会让单次偏慢）。 */
function measure(run: () => void, batches: number, perBatch: number): number {
  const medians: number[] = [];
  for (let b = 0; b < batches; b += 1) {
    const samples: number[] = [];
    for (let i = 0; i < perBatch; i += 1) {
      const t0 = performance.now();
      run();
      samples.push(performance.now() - t0);
    }
    medians.push(median(samples));
  }
  return Math.min(...medians);
}

const PARA = '这是一段用于基准测试的中文正文，包含 [[链接{i}]] 与 #标签/子 内容。';
const LARGE_PARA = '这是一段用于大文件基准测试的中文正文，包含 [[链接{i}]] 与 #标签/子 内容，用于逼近 2MB 量级。';
const docs = {
  small: buildDoc(400, PARA),
  // 门禁夹具（0.52MB）：与 DEBT-31 探针同规模，jsdom 下可稳定测量
  large: buildDoc(2500, LARGE_PARA),
  // 全量对照（2.5MB，AC-EDITOR-05 规模）：只在 KP_PERF_FULL=1 时测，需 ~8GB 堆
  full: buildDoc(12000, LARGE_PARA),
};

/**
 * 每次测量前清空渲染缓存：否则第二次起命中的是 LRU 缓存（实测会掉到 0.05ms），量到的不是管线开销。
 * **新路径与旧路径都不用缓存**，两者才可比。
 */
function renderFresh(doc: string): string {
  clearRenderCache();
  return renderMarkdown(doc);
}

/** 指标值表（key → 实测）。 */
interface MetricValue {
  value: number;
  docBytes: number;
}

/**
 * **门禁的判定依据**：把指标写成 JSON 文件，由 `scripts/run-perf.mjs` 读取。
 *
 * 为什么不靠 stdout 解析（2026-10-21 CI 真实事故）：`--reporter=dot` 的进度点是**不带换行**输出的，
 * 于是指标行可能被粘成 `·PERF_METRIC …`；脚本用 `^PERF_METRIC` 行首锚点解析时，这条指标就**静默丢失**，
 * 门禁只报"缺指标"（而那行又打在 stderr 上）—— 表现正是"large 通过、small 消失、退出码非零、没有解释"。
 * 写文件后，判定不再依赖"指标行是否恰好独占一行"这种脆弱前提。
 */
function writeMetricsFile(metrics: Record<string, MetricValue>): void {
  const outPath = process.env.KP_PERF_OUT;
  if (!outPath) return;
  mkdirSync(dirname(outPath), { recursive: true });
  const payload = { measuredAt: new Date().toISOString(), node: process.version, metrics };
  writeFileSync(outPath, JSON.stringify(payload, null, 2), 'utf8');
  console.log(`PERF_METRICS_FILE ${outPath}`);
}

function emit(prefix: string, key: string, value: number, bytes: number): void {
  // 给人看的行（日志友好）；**门禁判定不解析它** —— 见 writeMetricsFile 的说明。
  console.log(`${prefix}PERF_METRIC ${key} ${value.toFixed(3)} ${bytes}`);
}

describe('门禁 10：生产渲染管线（DEBT-31）', () => {
  it('先确认量的确实是生产管线（不是被换掉的快路径）', () => {
    const html = renderFresh(docs.large);
    // 三条特征分别来自：fence 高亮 / kp-inline 自研规则 / DOMPurify 之后的净化结果
    expect(html).toContain('class="kp-hl');
    expect(html).toContain('data-kp-link');
    expect(html).not.toContain('<script');
  });

  // 单次测量包含预热与 21 + 6 次完整渲染（jsdom 下每次几十到几百 ms）⇒ 必须放宽 vitest 的默认 5s 超时。
  it('小文档（约 67KB）与门禁夹具（约 0.5MB）各测一遍', { timeout: 180_000 }, () => {
    const bytes = { small: Buffer.byteLength(docs.small), large: Buffer.byteLength(docs.large) };
    // 预热（丢弃）：JIT 与 jsdom 的一次性初始化不计入
    measure(() => renderFresh(docs.small), 1, 2);
    measure(() => renderFresh(docs.large), 1, 1);

    if (process.env.KP_PERF_LEGACY === '1') {
      const legacy = new MarkdownIt({ html: false, linkify: false, typographer: false });
      emit('LEGACY_', 'markdownRenderSmall', measure(() => legacy.render(docs.small), 7, 20), bytes.small);
      emit('LEGACY_', 'markdownRenderLarge', measure(() => legacy.render(docs.large), 3, 3), bytes.large);
    }

    // 采样次数刻意压到"够用"：jsdom 每次渲染都会建/拆一棵 DOM，140 次小文档渲染实测会耗尽默认堆。
    // 估计量仍是「多批次中位数的最小值」，对机器噪声稳健。
    const values: Record<string, MetricValue> = {
      markdownRenderSmall: { value: measure(() => renderFresh(docs.small), 3, 5), docBytes: bytes.small },
      markdownRenderLarge: { value: measure(() => renderFresh(docs.large), 2, 3), docBytes: bytes.large },
    };
    // 两条指标一起写文件、一起打印（相邻输出也降低被 reporter 打断的概率）
    writeMetricsFile(values);
    for (const [key, item] of Object.entries(values)) emit('', key, item.value, item.docBytes);
    // 自检：两个门禁指标必须都在（少一个就说明探针自己坏了，别等门禁脚本去猜）
    expect(Object.keys(values).sort()).toEqual(['markdownRenderLarge', 'markdownRenderSmall']);
  });

  it.skipIf(process.env.KP_PERF_FULL !== '1')(
    '全量对照（2.5MB，AC-EDITOR-05 规模；需 --max-old-space-size=8192）',
    { timeout: 300_000 },
    () => {
      const bytes = Buffer.byteLength(docs.full);
      measure(() => renderFresh(docs.full), 1, 1);
      if (process.env.KP_PERF_LEGACY === '1') {
        const legacy = new MarkdownIt({ html: false, linkify: false, typographer: false });
        emit('FULL_LEGACY_', 'markdownRenderFull', measure(() => legacy.render(docs.full), 2, 2), bytes);
      }
      emit('FULL_', 'markdownRenderFull', measure(() => renderFresh(docs.full), 2, 2), bytes);
    },
  );
});