import { describe, expect, it } from 'vitest';
import { REQUIRED_CHECKS, backoffMs, evaluateChecks, isNetworkError, parseArgs, parseRepoFromUrl, tlsHint } from '../../scripts/pr-flow.mjs';

const run = (name, status, conclusion) => ({ name, status, conclusion });

describe('evaluateChecks', () => {
  it('必需检查全 success → 通过', () => {
    const runs = REQUIRED_CHECKS.map((n) => run(n, 'completed', 'success'));
    expect(evaluateChecks(runs).ok).toBe(true);
  });
  it('仍有排队/进行中 → 不通过且列入 pending', () => {
    const runs = [run(REQUIRED_CHECKS[0], 'completed', 'success'), run(REQUIRED_CHECKS[1], 'in_progress', null)];
    const v = evaluateChecks(runs);
    expect(v.ok).toBe(false);
    expect(v.pending).toEqual([REQUIRED_CHECKS[1]]);
  });
  it('失败 → 不通过且列入 failed', () => {
    const runs = [run(REQUIRED_CHECKS[0], 'completed', 'failure'), run(REQUIRED_CHECKS[1], 'completed', 'success')];
    const v = evaluateChecks(runs);
    expect(v.ok).toBe(false);
    expect(v.failed).toEqual([REQUIRED_CHECKS[0]]);
  });
  it('必需检查尚未出现 → 列入 missing', () => {
    const v = evaluateChecks([run(REQUIRED_CHECKS[0], 'completed', 'success')]);
    expect(v.ok).toBe(false);
    expect(v.missing).toEqual([REQUIRED_CHECKS[1]]);
  });
  it('skipped / neutral 视为通过（如平台条件跳过的步骤）', () => {
    const runs = [run(REQUIRED_CHECKS[0], 'completed', 'skipped'), run(REQUIRED_CHECKS[1], 'completed', 'neutral')];
    expect(evaluateChecks(runs).ok).toBe(true);
  });
  it('额外检查（如三平台冒烟）只作为 unknown 报告，不影响结论', () => {
    const runs = [...REQUIRED_CHECKS.map((n) => run(n, 'completed', 'success')), run('窗口启动冒烟（macos-latest）', 'completed', 'failure')];
    const v = evaluateChecks(runs);
    expect(v.ok).toBe(true);
    expect(v.unknown).toContain('窗口启动冒烟（macos-latest）');
  });
});

describe('backoffMs', () => {
  it('指数增长并封顶 20s', () => {
    expect(backoffMs(1)).toBe(1500);
    expect(backoffMs(2)).toBe(3000);
    expect(backoffMs(3)).toBe(6000);
    expect(backoffMs(10)).toBe(20000);
  });
});

describe('isNetworkError', () => {
  it('识别网络类错误（可重试）', () => {
    expect(isNetworkError("fatal: unable to access 'https://github.com/x/y.git/': The requested URL returned error: 504")).toBe(true);
    expect(isNetworkError('fatal: unable to access ...: Could not resolve host: github.com')).toBe(true);
    expect(isNetworkError('error: RPC failed; curl 56 Recv failure: Connection was reset')).toBe(true);
    expect(isNetworkError('fatal: The remote end hung up unexpectedly')).toBe(true);
  });
  it('业务类错误不重试（避免掩盖真实问题）', () => {
    expect(isNetworkError('remote: error: GH006: Protected branch update failed for refs/heads/main.')).toBe(false);
    expect(isNetworkError('! [rejected] main -> main (non-fast-forward)')).toBe(false);
    expect(isNetworkError('remote: Permission to x/y.git denied')).toBe(false);
  });
});

describe('tlsHint', () => {
  it('证书类错误给出 --use-system-ca 提示', () => {
    const hint = tlsHint({ cause: { code: 'UNABLE_TO_VERIFY_LEAF_SIGNATURE' } });
    expect(hint).toContain('--use-system-ca');
  });
  it('普通网络错误不误报为证书问题', () => {
    expect(tlsHint({ cause: { code: 'ECONNRESET' } })).toBeNull();
    expect(tlsHint(new Error('plain'))).toBeNull();
  });
});

describe('parseRepoFromUrl', () => {
  it('解析 https 形式（含 .git 后缀）', () => {
    expect(parseRepoFromUrl('https://github.com/softiy/Knowl-Pad.git')).toBe('softiy/Knowl-Pad');
    expect(parseRepoFromUrl('https://gitee.com/klincode/knowl-pad.git')).toBe('klincode/knowl-pad');
  });
  it('解析 ssh 形式', () => {
    expect(parseRepoFromUrl('git@github.com:softiy/Knowl-Pad.git')).toBe('softiy/Knowl-Pad');
  });
  it('非仓库 URL 返回 null', () => {
    expect(parseRepoFromUrl('https://example.com/x')).toBeNull();
    expect(parseRepoFromUrl('')).toBeNull();
  });
});

describe('parseArgs', () => {
  it('默认值', () => {
    const o = parseArgs([]);
    expect(o.command).toBe('run');
    expect(o.wait).toBe(20);
    expect(o.merge).toBe(true);
    expect(o.verifyMirror).toBe(true);
  });
  it('解析命令与数值选项', () => {
    const o = parseArgs(['open', '--wait', '3', '--interval', '5', '--retries', '2']);
    expect(o.command).toBe('open');
    expect(o.wait).toBe(3);
    expect(o.interval).toBe(5);
    expect(o.retries).toBe(2);
  });
  it('--no-merge / --no-mirror 关闭开关', () => {
    const o = parseArgs(['--no-merge', '--no-mirror']);
    expect(o.merge).toBe(false);
    expect(o.verifyMirror).toBe(false);
  });
  it('未知选项抛错（避免静默忽略拼错的参数）', () => {
    expect(() => parseArgs(['--wiat', '3'])).toThrow();
  });
});
