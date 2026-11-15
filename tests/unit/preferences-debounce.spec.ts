/** 偏好写入的防抖批量提交（PRD FR-SET-03 / PERF-06；勘误 DEBT-24③）。 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({ preferenceSet: vi.fn(async () => undefined) }));

import { preferenceSet } from '@core/ipc/commands';
import {
  PREFERENCE_FLUSH_MS,
  flushPreferences,
  installPreferenceFlushOnUnload,
  pendingPreferenceCount,
  preferenceFlushError,
  schedulePreferenceSet,
} from '@core/ipc/preferences';

const mSet = vi.mocked(preferenceSet);

beforeEach(async () => {
  vi.useFakeTimers();
  mSet.mockReset();
  mSet.mockResolvedValue(undefined);
  await flushPreferences();
  mSet.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe(
  '防抖批量提交（500ms 窗口）',
  () => {
    it(
      '同一窗口内的多个键合成一次 IPC，且窗口按最后一次调用重置',
      async () => {
        schedulePreferenceSet({ a: 1 });
        await vi.advanceTimersByTimeAsync(PREFERENCE_FLUSH_MS - 100);
        expect(mSet).not.toHaveBeenCalled();
        schedulePreferenceSet({ b: 2 });
        // 距第一次已 400ms，但最后一次调用只过了 400ms ⇒ 还不该提交
        await vi.advanceTimersByTimeAsync(PREFERENCE_FLUSH_MS - 100);
        expect(mSet).not.toHaveBeenCalled();
        await vi.advanceTimersByTimeAsync(100);
        expect(mSet).toHaveBeenCalledTimes(1);
        expect(mSet).toHaveBeenCalledWith({ a: 1, b: 2 });
        expect(pendingPreferenceCount()).toBe(0);
      },
    );

    it(
      '同一个键被改多次时，只提交最后一次的值',
      async () => {
        schedulePreferenceSet({ k: 1 });
        schedulePreferenceSet({ k: 2 });
        await vi.advanceTimersByTimeAsync(PREFERENCE_FLUSH_MS);
        expect(mSet).toHaveBeenCalledTimes(1);
        expect(mSet).toHaveBeenCalledWith({ k: 2 });
      },
    );

    it(
      'flushPreferences 立即提交（不等窗口），空队列是空操作',
      async () => {
        await flushPreferences();
        expect(mSet).not.toHaveBeenCalled();
        schedulePreferenceSet({ a: 1 });
        await flushPreferences();
        expect(mSet).toHaveBeenCalledTimes(1);
        expect(mSet).toHaveBeenCalledWith({ a: 1 });
        expect(pendingPreferenceCount()).toBe(0);
      },
    );

    it(
      '提交失败时把条目放回队列并记下原因，恢复后能重试成功',
      async () => {
        mSet.mockRejectedValueOnce(new Error('磁盘忙'));
        schedulePreferenceSet({ a: 1 });
        await flushPreferences();
        expect(pendingPreferenceCount()).toBe(1);
        expect(preferenceFlushError()).toContain('磁盘忙');
        await flushPreferences();
        expect(mSet).toHaveBeenCalledTimes(2);
        expect(mSet).toHaveBeenLastCalledWith({ a: 1 });
        expect(pendingPreferenceCount()).toBe(0);
        expect(preferenceFlushError()).toBeNull();
      },
    );
  },
);
/**
 * S2 的另一半根因：偏好写入是 500ms 防抖批量提交，用户改完设置立刻关窗时定时器还没到点，
 * 队列会随进程退出一起消失（重启后"回到默认值"）。窗口卸载/隐藏前必须 flush。
 */
describe('卸载/隐藏时提交（S2 回归）', () => {
  let uninstall: (() => void) | null = null;

  afterEach(() => {
    uninstall?.();
    uninstall = null;
    // 恢复 jsdom 默认的 visibilityState getter（见下方 stubVisibility）
    delete (document as { visibilityState?: DocumentVisibilityState }).visibilityState;
    vi.restoreAllMocks();
  });

  /** jsdom 的 visibilityState 是只读 getter；用可配置实例属性模拟"页面被隐藏"。 */
  function stubVisibility(state: DocumentVisibilityState): void {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state });
  }

  it('beforeunload 时立即提交队列，不等 500ms 窗口', async () => {
    uninstall = installPreferenceFlushOnUnload();
    schedulePreferenceSet({ 'appearance.theme': 'dark' });
    window.dispatchEvent(new Event('beforeunload'));
    await vi.advanceTimersByTimeAsync(0);
    expect(mSet).toHaveBeenCalledTimes(1);
    expect(mSet).toHaveBeenCalledWith({ 'appearance.theme': 'dark' });
    expect(pendingPreferenceCount()).toBe(0);
  });

  it('页面可见时不提交；切到 hidden 时提交（最小化/切后台被回收的路径）', async () => {
    uninstall = installPreferenceFlushOnUnload();
    stubVisibility('visible');
    schedulePreferenceSet({ 'appearance.editorFontSize': 18 });
    document.dispatchEvent(new Event('visibilitychange'));
    await vi.advanceTimersByTimeAsync(0);
    expect(mSet).not.toHaveBeenCalled();
    stubVisibility('hidden');
    document.dispatchEvent(new Event('visibilitychange'));
    await vi.advanceTimersByTimeAsync(0);
    expect(mSet).toHaveBeenCalledWith({ 'appearance.editorFontSize': 18 });
    expect(pendingPreferenceCount()).toBe(0);
  });

  it('重复 install 只注册一份监听；注销后标记复位（可再次安装）', () => {
    const spy = vi.spyOn(window, 'addEventListener');
    const beforeUnloadRegistrations = (): number =>
      spy.mock.calls.filter(([type]) => type === 'beforeunload').length;

    uninstall = installPreferenceFlushOnUnload();
    const second = installPreferenceFlushOnUnload();
    expect(beforeUnloadRegistrations(), '重复 install 不得重复注册').toBe(1);
    second();

    uninstall();
    uninstall = installPreferenceFlushOnUnload();
    expect(beforeUnloadRegistrations(), '注销后允许再次安装').toBe(2);
  });
});
