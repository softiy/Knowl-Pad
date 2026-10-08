import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

/**
 * 链接 store（FR-LINK-17：数据必须来自索引库**实时查询**，事件后 1 秒内刷新）。
 */
const hoisted = vi.hoisted(() => ({
  dangling: vi.fn(),
  ambiguous: vi.fn(),
  orphans: vi.fn(),
  backlinks: vi.fn(),
  handlers: [] as (() => void)[],
}));

vi.mock('@core/ipc/linkCommands', () => ({
  linkDanglingList: hoisted.dangling,
  linkAmbiguousList: hoisted.ambiguous,
  linkOrphanList: hoisted.orphans,
  linkBacklinks: hoisted.backlinks,
}));
vi.mock('@core/ipc/events', () => {
  const register = (h: () => void) => {
    hoisted.handlers.push(h);
    return async () => {};
  };
  return {
    onIndexCompleted: async (h: () => void) => register(h),
    onFsCreated: async (h: () => void) => register(h),
    onFsModified: async (h: () => void) => register(h),
    onFsRemoved: async (h: () => void) => register(h),
    onFsRenamed: async (h: () => void) => register(h),
  };
});

import { useLinksStore } from '@features/links/stores/links';

describe('链接面板 store', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.handlers.length = 0;
    hoisted.dangling.mockReset().mockResolvedValue({ items: [{ targetRef: 'x', refCount: 2, sourceCount: 1, sampleSources: ['a.md'] }], total: 1 });
    hoisted.ambiguous.mockReset().mockResolvedValue({ items: [], total: 0 });
    hoisted.orphans.mockReset().mockResolvedValue({ items: ['solo.md'], total: 1 });
    hoisted.backlinks
      .mockReset()
      .mockResolvedValue([{ srcRelPath: 'a.md', srcName: 'a.md', linkCount: 2, embedCount: 1, items: [] }]);
  });

  it('刷新即向索引库实时查询，并给出各类计数', async () => {
    const s = useLinksStore();
    await s.refresh();
    expect(hoisted.dangling).toHaveBeenCalledTimes(1);
    expect(hoisted.ambiguous).toHaveBeenCalledTimes(1);
    expect(hoisted.orphans).toHaveBeenCalledTimes(1);
    expect(s.totals.dangling).toBe(1);
    expect(s.totals.orphans).toBe(1);
    expect(s.lastRefreshedAt).toBeGreaterThan(0);
  });

  it('设置当前笔记后才查询反链，并区分普通与嵌入计数', async () => {
    const s = useLinksStore();
    await s.setActive('b.md');
    expect(hoisted.backlinks).toHaveBeenCalledWith('b.md');
    expect(s.totals.backlinks).toBe(2);
    expect(s.totals.embeds).toBe(1);
    await s.setActive(null);
    expect(s.backlinks).toEqual([]);
  });

  it('事件到来后防抖刷新（1 秒内可见），且不重复注册', async () => {
    vi.useFakeTimers();
    const s = useLinksStore();
    await s.subscribe();
    expect(hoisted.handlers.length).toBe(5);
    await s.refresh();
    const before = hoisted.dangling.mock.calls.length;
    for (const h of hoisted.handlers) h();
    for (const h of hoisted.handlers) h();
    await vi.advanceTimersByTimeAsync(300);
    expect(hoisted.dangling.mock.calls.length).toBe(before + 1);
    s.dispose();
    vi.useRealTimers();
  });

  it('查询失败时给出可见错误而不是抛给渲染层（R-15）', async () => {
    hoisted.dangling.mockRejectedValue(new Error('boom'));
    const s = useLinksStore();
    await s.refresh();
    expect(s.error).toBeTruthy();
    expect(s.loading).toBe(false);
  });
});
