// 门禁 10 的**基线环境维度**（DEBT-31 的 #160 后续 / 第一步）。
//
// 为什么需要：绝对耗时跨机器不可比 —— 本机 24 逻辑核 Windows 与 CI 共享 Ubuntu 2 vCPU 的同一份数字，
// 比较本身没有意义。方案是「每个环境一份基线」，并且**绝不跨环境回退**：当前环境没基线就硬失败（exit 2）
// 并告诉你怎么建立，而不是偷偷用别的环境的数字（那等于让门禁自己给自己发通行证）。
//
// 独立性：这里只放**纯函数**（环境名解析、基线读写），判定与阈值仍留在 `run-perf.mjs`；
// `node scripts/run-perf.mjs --self-test` 会直接跑本文件的自检。

/** 基线格式版本（v3 = 带 envs）。 */
export const BASELINE_VERSION = 3;

/**
 * 解析"当前环境"的名字（决定读/写哪份基线）。
 *
 * 优先级：
 * 1. `KP_PERF_ENV`（显式指定；CI 工作流里钉死，避免 runner 规格变化导致基线漂移）
 * 2. `CI` 为真 ⇒ `ci-ubuntu-2vcpu`（**刻意不用 CPU 数**：runner 偶尔换规格就会换名字，基线会莫名其妙地"缺"）
 * 3. 本地 ⇒ `local-<os>-<N>c`（如 `local-win-24c`、`local-macos-8c`）
 */
export function resolveEnvName({ env = {}, platform = process.platform, cpuCount = 1 } = {}) {
  const explicit = String(env.KP_PERF_ENV ?? '').trim();
  if (explicit) return explicit;
  if (env.CI) return 'ci-ubuntu-2vcpu';
  const os = platform === 'win32' ? 'win' : platform === 'darwin' ? 'macos' : 'linux';
  return `local-${os}-${cpuCount}c`;
}

/** 基线格式错误：带 `detail` 行数组，供 `fail()` 直接打印。 */
function formatError(message, detail) {
  const err = new Error(message);
  err.detail = detail;
  return err;
}

/**
 * 解析基线文件（只认 v3）。旧格式**不自动迁移**：迁移涉及"这些数字是哪台机器量的"，
 * 自动猜环境会把别的环境的数字写进本环境，正是这套机制要防的事。
 */
export function parseBaseline(raw) {
  const parsed = JSON.parse(raw);
  if (parsed && parsed.version === BASELINE_VERSION && typeof parsed.envs === 'object' && parsed.envs !== null) {
    return parsed;
  }
  throw formatError('基线格式不是 v3（缺少 envs）', [
    '文件头 version=' + String(parsed?.version) + '（期望 ' + BASELINE_VERSION + '）',
    '迁移方式：在**该数字所属的机器**上跑 pnpm test:perf -- --update-baseline（会写成 envs[<环境名>]）',
    '或手工把旧 metrics 移进 envs.<环境名>.metrics（环境名见本脚本每轮打印的那行）',
  ]);
}

/** 取某环境的基线块；没有则 null（**不回退**到别的环境）。 */
export function envBlock(baseline, envName) {
  if (!baseline || typeof baseline.envs !== 'object' || baseline.envs === null) return null;
  return baseline.envs[envName] ?? null;
}

/** 已有环境名列表（诊断用）。 */
export function envNames(baseline) {
  return Object.keys(baseline?.envs ?? {});
}

/** 把某环境的指标写进基线（保留其它环境的块）。 */
export function setEnvMetrics(baseline, envName, metrics, nowIso) {
  return {
    version: BASELINE_VERSION,
    pipeline: baseline?.pipeline ?? null,
    envs: { ...(baseline?.envs ?? {}), [envName]: { metrics, updatedAt: nowIso } },
  };
}

/** 自检（`node scripts/run-perf.mjs --self-test`）：纯函数行为 + 失败路径都必须被钉住。 */
export function selfTest(log = console.log) {
  const cases = [];
  const check = (name, fn) => {
    try {
      fn();
      cases.push([name, true]);
    } catch (err) {
      cases.push([name, false, String(err && err.message ? err.message : err)]);
    }
  };
  const eq = (actual, expected) => {
    if (actual !== expected) throw new Error(`期望 ${expected}，实际 ${actual}`);
  };

  check('KP_PERF_ENV 显式指定优先', () => eq(resolveEnvName({ env: { KP_PERF_ENV: 'ci-ubuntu-2vcpu', CI: 'true' } }), 'ci-ubuntu-2vcpu'));
  check('CI 环境名固定（不随 CPU 数变化）', () => eq(resolveEnvName({ env: { CI: 'true' }, cpuCount: 8 }), 'ci-ubuntu-2vcpu'));
  check('本地 Windows 按核数命名', () => eq(resolveEnvName({ env: {}, platform: 'win32', cpuCount: 24 }), 'local-win-24c'));
  check('本地 macOS 命名', () => eq(resolveEnvName({ env: {}, platform: 'darwin', cpuCount: 8 }), 'local-macos-8c'));
  check('空白 KP_PERF_ENV 视为未设置', () => eq(resolveEnvName({ env: { KP_PERF_ENV: '   ' }, platform: 'linux', cpuCount: 4 }), 'local-linux-4c'));

  const v3 = { version: 3, pipeline: 'production', envs: { 'local-win-24c': { metrics: { a: 1 } } } };
  check('v3 基线可解析', () => eq(parseBaseline(JSON.stringify(v3)).version, 3));
  check('v2 基线被拒绝且给出迁移指引', () => {
    let err = null;
    try { parseBaseline(JSON.stringify({ version: 2, metrics: {} })); } catch (e) { err = e; }
    if (!err) throw new Error('v2 没有被拒绝');
    if (!Array.isArray(err.detail) || err.detail.length === 0) throw new Error('缺少迁移指引');
  });
  check('缺当前环境时 envBlock 返回 null（不回退）', () => eq(envBlock(v3, 'ci-ubuntu-2vcpu'), null));
  check('envBlock 命中', () => eq(envBlock(v3, 'local-win-24c').metrics.a, 1));
  check('setEnvMetrics 保留其它环境', () => {
    const next = setEnvMetrics(v3, 'ci-ubuntu-2vcpu', { a: 2 }, '2026-10-21T00:00:00.000Z');
    eq(next.envs['local-win-24c'].metrics.a, 1);
    eq(next.envs['ci-ubuntu-2vcpu'].metrics.a, 2);
    eq(next.pipeline, 'production');
  });
  check('空基线也能写入首个环境', () => {
    const next = setEnvMetrics({ version: 3, pipeline: 'production', envs: {} }, 'local-win-24c', { a: 3 }, 'x');
    eq(next.envs['local-win-24c'].metrics.a, 3);
  });

  const failed = cases.filter(([, ok]) => !ok);
  for (const [name, ok, msg] of cases) log(`${ok ? '✅' : '❌'} ${name}${msg ? '：' + msg : ''}`);
  log(failed.length === 0 ? '✅ 基线环境维度自检通过' : '❌ 自检失败 ' + failed.length + ' 项');
  return failed.length === 0;
}