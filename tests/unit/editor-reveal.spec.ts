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
import { revealLineIn } from '@features/editor/adapter/revealLine';

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
  it('未提供 onDegrade 时走内置默认：提示一次、不抛错、请求被消费', () => {
    // 审计 T-04：这条此前是一个恒真断言（字面量真值比较），等于什么都没验证。
    // 现在真的断言内置默认行为：console.warn 被调用一次，且请求被清除（不假装定位过）。
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    let cleared = 0;
    const stop = consumeReveal({
      current: () => ({ relPath: 'a.md', line: 3 }),
      active: () => 'a.md',
      clear: () => {
        cleared += 1;
      },
      adapter: () => ({ revealLine: () => false }),
    });
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0] ?? '')).toContain('行定位');
    expect(cleared).toBe(1);
    stop();
    warn.mockRestore();
  });
});
describe('列级定位（FR-SEARCH-07 的"跳到命中处"）', () => {
  /** 真 DOM：把 textarea 放进宿主，断言**实际**选中的偏移，而不是"应该能"。 */
  function hostWith(value: string): { root: HTMLElement; ta: HTMLTextAreaElement } {
    const root = document.createElement('div');
    const ta = document.createElement('textarea');
    ta.value = value;
    root.appendChild(ta);
    document.body.appendChild(root);
    return { root, ta };
  }

  const VALUE = '第一行\nsecond line here\n第三行';
  // 第 1 行 3 个字符 + \n ⇒ 第 2 行从下标 4 开始
  const LINE2 = 4;

  it('给了列区间 ⇒ 精确选中命中处（不是整行）', () => {
    const { root, ta } = hostWith(VALUE);
    expect(revealLineIn(root, VALUE, 2, { start: 7, end: 11 })).toBe(true);
    expect(ta.selectionStart).toBe(LINE2 + 7);
    expect(ta.selectionEnd).toBe(LINE2 + 11);
    expect(VALUE.slice(ta.selectionStart, ta.selectionEnd)).toBe('line');
  });

  it('不给区间 ⇒ 仍选中整行（反链 FR-LINK-13 的既有行为不变）', () => {
    const { root, ta } = hostWith(VALUE);
    expect(revealLineIn(root, VALUE, 2)).toBe(true);
    expect(ta.selectionStart).toBe(LINE2);
    expect(ta.selectionEnd).toBe(LINE2 + 'second line here'.length);
  });

  it('区间越界/为空/反向 ⇒ 一律退回整行（绝不选空）', () => {
    const { root, ta } = hostWith(VALUE);
    // 实测行为：两端都夹到行尾后区间为空 ⇒ 走"退回整行"分支（不是"选中行尾的空区间"）
    revealLineIn(root, VALUE, 2, { start: 999, end: 9999 });
    expect(ta.selectionStart).toBe(LINE2);
    expect(ta.selectionEnd).toBe(LINE2 + 'second line here'.length);

    revealLineIn(root, VALUE, 2, { start: 5, end: 5 });
    expect(ta.selectionStart).toBe(LINE2); // 空区间 ⇒ 退回整行
    expect(ta.selectionEnd).toBe(LINE2 + 'second line here'.length);

    revealLineIn(root, VALUE, 2, { start: 9, end: 3 });
    expect(ta.selectionStart).toBe(LINE2); // 反向 ⇒ 退回整行
    expect(ta.selectionEnd).toBe(LINE2 + 'second line here'.length);
  });

  it('行号越界仍按既有规则收敛，且区间照样生效', () => {
    const { root, ta } = hostWith(VALUE);
    revealLineIn(root, VALUE, 99, { start: 0, end: 1 });
    expect(VALUE.slice(ta.selectionStart, ta.selectionEnd)).toBe('第');
  });

  it('没有 textarea 的宿主 ⇒ 返回 false（如实降级，不假装选中）', () => {
    const root = document.createElement('div');
    expect(revealLineIn(root, VALUE, 1, { start: 0, end: 1 })).toBe(false);
  });

  it('consumeReveal 会把列区间一并交给适配器', () => {
    const seen: Array<{ line: number; range?: { start: number; end: number } }> = [];
    let req: { relPath: string; line: number; range?: { start: number; end: number } } | null = {
      relPath: 'a.md',
      line: 5,
      range: { start: 2, end: 6 },
    };
    const stop = consumeReveal({
      current: () => req,
      active: () => 'a.md',
      clear: () => {
        req = null;
      },
      adapter: () => ({
        revealLine: (line: number, range?: { start: number; end: number }) => {
          seen.push(range ? { line, range } : { line });
          return true;
        },
      }),
    });
    expect(seen).toEqual([{ line: 5, range: { start: 2, end: 6 } }]);
    stop();
  });

  it('store.reveal 带区间时写进请求；不带时保持原形状（既有断言不破）', async () => {
    const store = useEditorStore();
    await store.reveal('a.md', 7, { start: 1, end: 3 });
    expect(store.pendingReveal).toEqual({ relPath: 'a.md', line: 7, range: { start: 1, end: 3 } });
    await store.reveal('a.md', 8);
    expect(store.pendingReveal).toEqual({ relPath: 'a.md', line: 8 });
  });
});
