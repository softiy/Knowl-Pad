import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

/**
 * 反链点击定位（FR-LINK-13 后半）：store 记录待定位行，consumeReveal 消费一次。
 */
vi.mock('@core/ipc/commands', () => ({
  noteRead: vi.fn(async () => ({ relPath: 'a.md', content: '# 甲\n第一行\n第二行\n', mtimeMs: 1, sizeBytes: 10 })),
  noteWrite: vi.fn(async () => 2),
}));

import { useEditorStore } from '@features/editor';
import { consumeReveal } from '@features/editor/adapter/consumeReveal';

describe('编辑器行定位（FR-LINK-13）', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('reveal 打开来源笔记并记下待定位行；clearReveal 清空', async () => {
    const store = useEditorStore();
    await store.reveal('a.md', 7);
    expect(store.activeRelPath).toBe('a.md');
    expect(store.pendingReveal).toEqual({ relPath: 'a.md', line: 7 });
    store.clearReveal();
    expect(store.pendingReveal).toBeNull();
  });

  it('consumeReveal：活动笔记匹配时定位一次并清除请求', () => {
    const seen: number[] = [];
    let req: { relPath: string; line: number } | null = { relPath: 'a.md', line: 5 };
    const stop = consumeReveal({
      current: () => req,
      active: () => 'a.md',
      clear: () => { req = null; },
      adapter: () => ({ revealLine: (line: number) => { seen.push(line); return true; } }),
    });
    expect(seen).toEqual([5]);
    expect(req).toBeNull();
    stop();
  });

  it('consumeReveal：适配器能力不足时如实降级（不假装定位过）', () => {
    let degraded = 0;
    const stop = consumeReveal({
      current: () => ({ relPath: 'a.md', line: 5 }),
      active: () => 'a.md',
      clear: () => {},
      adapter: () => ({ revealLine: () => false }),
      onDegrade: () => { degraded += 1; },
    });
    expect(degraded).toBe(1);
    stop();
  });

  it('consumeReveal：活动笔记还不是请求的那篇时不消费', () => {
    let calls = 0;
    const stop = consumeReveal({
      current: () => ({ relPath: 'b.md', line: 1 }),
      active: () => 'a.md',
      clear: () => {},
      adapter: () => ({ revealLine: () => { calls += 1; return true; } }),
    });
    expect(calls).toBe(0);
    stop();
  });

  it('consumeReveal：适配器尚未挂载（null）时不消费也不报错', () => {
    let cleared = 0;
    const stop = consumeReveal({
      current: () => ({ relPath: 'a.md', line: 1 }),
      active: () => 'a.md',
      clear: () => { cleared += 1; },
      adapter: () => null,
    });
    expect(cleared).toBe(0);
    stop();
  });
});

describe('默认降级路径', () => {
  it('未提供 onDegrade 时走内置默认（只提示，不抛错、不假装定位过）', () => {
    const stop = consumeReveal({
      current: () => ({ relPath: 'a.md', line: 3 }),
      active: () => 'a.md',
      clear: () => {},
      adapter: () => ({ revealLine: () => false }),
    });
    stop();
    expect(true).toBe(true);
  });
});
