import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

/**
 * 编辑器行定位（FR-LINK-13）：
 * - store 记下"待定位行"，consumeReveal 消费一次；
 * - 定位算法是**纯函数**（行号/列区间 → 文档偏移），在 CodeMirror 视图上执行原生选区事务。
 *
 * 旧实现走 `textarea.setSelectionRange`，而 md-editor-v3 6.x 的内核是 CodeMirror 6、没有 textarea
 * ⇒ 永远定位失败，只计数不选中（10.1/10.2/10.4）。这里用结构化替身锁住新口径。
 */
vi.mock('@core/ipc/commands', () => ({
  noteRead: vi.fn(async () => ({ relPath: 'a.md', content: '# 甲\n第一行\n第二行\n', mtimeMs: 1, sizeBytes: 10 })),
  noteWrite: vi.fn(async () => 2),
}));

import { useEditorStore } from '@features/editor';
import { consumeReveal } from '@features/editor/adapter/consumeReveal';
import { editorViewOf, revealInView, type EditorViewLike } from '@features/editor/adapter/editorView';
import { revealOffsets } from '@features/editor/adapter/revealLine';

/** 与 CodeMirror `Text` 同口径的最小文档替身。 */
function docOf(text: string) {
  const rows = text.split('\n');
  const starts: number[] = [];
  let offset = 0;
  for (const row of rows) {
    starts.push(offset);
    offset += row.length + 1;
  }
  return {
    lines: rows.length,
    line(number: number): { from: number; to: number } {
      const from = starts[number - 1] ?? 0;
      return { from, to: from + (rows[number - 1] ?? '').length };
    },
    toString: (): string => text,
  };
}

function viewOf(text: string): EditorViewLike & { selections: { anchor: number; head: number }[]; scrolls: number; focusCalls: number } {
  const fake = {
    selections: [] as { anchor: number; head: number }[],
    scrolls: 0,
    focusCalls: 0,
    state: { doc: docOf(text) },
    dispatch(spec: {
      selection?: { anchor: number; head: number } | undefined;
      scrollIntoView?: boolean | undefined;
      changes?: readonly { from: number; to: number; insert: string }[] | undefined;
    }): void {
      if (spec.selection) fake.selections.push(spec.selection);
      if (spec.scrollIntoView === true) fake.scrolls += 1;
    },
    focus(): void {
      fake.focusCalls += 1;
    },
  };
  return fake;
}

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
      clear: () => {
        req = null;
      },
      adapter: () => ({
        revealLine: (line: number) => {
          seen.push(line);
          return true;
        },
      }),
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
      onDegrade: () => {
        degraded += 1;
      },
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
      adapter: () => ({
        revealLine: () => {
          calls += 1;
          return true;
        },
      }),
    });
    expect(calls).toBe(0);
    stop();
  });

  it('consumeReveal：适配器尚未挂载（null）时不消费也不报错', () => {
    let cleared = 0;
    const stop = consumeReveal({
      current: () => ({ relPath: 'a.md', line: 1 }),
      active: () => 'a.md',
      clear: () => {
        cleared += 1;
      },
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

describe('行号/列区间 → 文档偏移（FR-LINK-13 / FR-SEARCH-07 的纯计算）', () => {
  const VALUE = '第一行\nsecond line here\n第三行';
  // 第 1 行 3 个字符 + 换行符 ⇒ 第 2 行从下标 4 开始
  const LINE2 = 4;

  it('整行：从行首到行尾（含空行则 from === to）', () => {
    const doc = docOf(VALUE);
    expect(revealOffsets(doc, 1)).toEqual({ from: 0, to: 3 });
    expect(revealOffsets(doc, 2)).toEqual({ from: LINE2, to: LINE2 + 'second line here'.length });
    const withEmpty = docOf('a\n\nb');
    expect(revealOffsets(withEmpty, 2)).toEqual({ from: 2, to: 2 });
  });

  it('列区间：精确选中命中片段（不是整行）', () => {
    expect(revealOffsets(docOf(VALUE), 2, { start: 7, end: 11 })).toEqual({ from: LINE2 + 7, to: LINE2 + 11 });
  });

  it('区间越界/为空/反向 ⇒ 一律退回整行（绝不选空）', () => {
    const doc = docOf(VALUE);
    const whole = { from: LINE2, to: LINE2 + 'second line here'.length };
    expect(revealOffsets(doc, 2, { start: 999, end: 9999 })).toEqual(whole);
    expect(revealOffsets(doc, 2, { start: 5, end: 5 })).toEqual(whole);
    expect(revealOffsets(doc, 2, { start: 9, end: 3 })).toEqual(whole);
    // 负值夹到行首；两端夹完为空同样退回整行
    expect(revealOffsets(doc, 2, { start: -5, end: -1 })).toEqual(whole);
  });

  it('行号越界收敛到首/末行；空文档（lines<1）返回 null 如实降级', () => {
    const doc = docOf(VALUE);
    expect(revealOffsets(doc, 0)).toEqual({ from: 0, to: 3 });
    expect(revealOffsets(doc, 99)).toEqual({ from: VALUE.indexOf('第三行'), to: VALUE.indexOf('第三行') + 3 });
    expect(revealOffsets({ lines: 0, line: () => ({ from: 0, to: 0 }) }, 1)).toBeNull();
  });
});

describe('CodeMirror 视图接面（narrow + 选区事务）', () => {
  it('editorViewOf：形状不符（旧内核/未就绪）一律返回 null，而不是抛错', () => {
    expect(editorViewOf(undefined)).toBeNull();
    expect(editorViewOf(null)).toBeNull();
    expect(editorViewOf({})).toBeNull();
    expect(editorViewOf({ getEditorView: () => undefined })).toBeNull();
    expect(editorViewOf({ getEditorView: () => ({ state: { doc: {} }, dispatch: () => {}, focus: () => {} }) })).toBeNull();
    expect(editorViewOf({ getEditorView: () => ({ focus: () => {} }) })).toBeNull();
    const ok = editorViewOf({ getEditorView: () => viewOf('a') });
    expect(ok).not.toBeNull();
  });

  it('revealInView：一次事务同时设置选区并滚动到视口', () => {
    const view = viewOf('第一行\nsecond line here\n第三行');
    expect(revealInView(view, 2, { start: 7, end: 11 })).toBe(true);
    expect(view.selections).toEqual([{ anchor: 4 + 7, head: 4 + 11 }]);
    expect(view.scrolls).toBe(1);
  });

  it('revealInView：文档不可用时返回 false（调用方据此降级，不假装定位过）', () => {
    const empty: EditorViewLike = {
      state: { doc: { lines: 0, line: () => ({ from: 0, to: 0 }), toString: () => '' } },
      dispatch: () => {},
      focus: () => {},
    };
    expect(revealInView(empty, 1)).toBe(false);
  });
});

describe('列级定位（FR-SEARCH-07 的"跳到命中处"）', () => {
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
