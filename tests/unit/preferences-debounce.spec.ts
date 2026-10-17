/** 偏好写入的防抖批量提交（PRD FR-SET-03 / PERF-06；勘误 DEBT-24③）。 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({ preferenceSet: vi.fn(async () => undefined) }));

import { preferenceSet } from '@core/ipc/commands';
import {
  PREFERENCE_FLUSH_MS,
  flushPreferences,
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
