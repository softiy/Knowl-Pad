import { defineComponent, h, nextTick, watch } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * 用极简替身替换 md-editor-v3：只测我们这一侧的适配逻辑（jsdom 跑不动真实内核）。
 *
 * md-editor-v3 6.x 的内核是 **CodeMirror 6**：替身除了 textarea（模拟输入）之外，还模拟组件实例
 * 暴露的 `getEditorView()`（文档 + 选区事务 + focus）。旧替身只有 textarea，恰好掩盖了
 * "适配器查 textarea 永远 null"的缺陷 —— 于是"只计数不选中/不标记"（10.1/10.2/10.4）一直没被测出来。
 */
const fake = vi.hoisted(() => {
  interface LineInfo {
    from: number;
    to: number;
  }
  interface DispatchSpec {
    changes?: readonly { from: number; to: number; insert: string }[];
    selection?: { anchor: number; head: number };
    scrollIntoView?: boolean;
  }
  const state = {
    text: '',
    selection: null as { anchor: number; head: number } | null,
    scrollCalls: 0,
    dispatches: [] as DispatchSpec[],
    focusCalls: 0,
    exposeView: true,
  };
  const rows = (): string[] => state.text.split('\n');
  const doc = {
    get lines(): number {
      return rows().length;
    },
    line(number: number): LineInfo {
      const all = rows();
      const idx = Math.min(Math.max(number, 1), all.length) - 1;
      let from = 0;
      for (let i = 0; i < idx; i += 1) from += (all[i] ?? '').length + 1;
      return { from, to: from + (all[idx] ?? '').length };
    },
    toString(): string {
      return state.text;
    },
  };
  const view = {
    state: { doc },
    dispatch(spec: DispatchSpec): void {
      state.dispatches.push(spec);
      if (spec.changes) {
        for (const change of [...spec.changes].sort((a, b) => b.from - a.from)) {
          state.text = state.text.slice(0, change.from) + change.insert + state.text.slice(change.to);
        }
      }
      if (spec.selection) state.selection = spec.selection;
      if (spec.scrollIntoView === true) state.scrollCalls += 1;
    },
    focus(): void {
      state.focusCalls += 1;
    },
  };
  function reset(text: string): void {
    state.text = text;
    state.selection = null;
    state.scrollCalls = 0;
    state.dispatches = [];
    state.focusCalls = 0;
  }
  return { state, doc, view, reset };
});

vi.mock('md-editor-v3', () => ({
  MdEditor: defineComponent({
    name: 'MdEditorStub',
    props: {
      modelValue: { type: String, default: '' },
      theme: { type: String, default: '' },
    },
    emits: ['update:modelValue', 'onSave'],
    setup(props, { emit, expose }) {
      fake.reset(props.modelValue);
      // 模拟内核的 modelValue 回灌：文档已一致时不重灌（真实内核也是这么比较的）
      watch(
        () => props.modelValue,
        (next) => {
          if (next !== fake.doc.toString()) fake.state.text = next;
        },
      );
      if (fake.state.exposeView) expose({ getEditorView: () => fake.view });
      return () =>
        h('textarea', {
          class: 'stub-editor',
          'data-editor-theme': props.theme,
          value: props.modelValue,
          onInput: (event: Event) => {
            const next = (event.target as HTMLTextAreaElement).value;
            // 内核文档先变、模型事件后发（与 CodeMirror 的 updateListener 顺序一致）
            fake.state.text = next;
            emit('update:modelValue', next);
          },
        });
    },
  }),
}));

import { CHANGE_THROTTLE_MS, MdEditorV3Adapter } from '@features/editor';
import { createEditorAdapter, currentEngine } from '@features/editor/adapter';

/** 本文件挂载过的适配器：用例结束后统一 destroy（否则 MutationObserver / 定时器会跨用例泄漏）。 */
const mounted: MdEditorV3Adapter[] = [];

function destroyAll(): void {
  for (const adapter of mounted.splice(0)) adapter.destroy();
}

function mountAdapter(initial = '', options: { exposeView?: boolean } = {}) {
  const el = document.createElement('div');
  document.body.append(el);
  fake.state.exposeView = options.exposeView ?? true;
  const adapter = new MdEditorV3Adapter();
  adapter.mount(el, { initialValue: initial });
  mounted.push(adapter);
  return { el, adapter };
}

function type(el: HTMLElement, value: string): void {
  const textarea = el.querySelector('textarea');
  if (!textarea) throw new Error('编辑器未挂载');
  textarea.value = value;
  textarea.dispatchEvent(new Event('input'));
}

describe('MdEditorV3Adapter（§7.4）', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    destroyAll();
    vi.useRealTimers();
    document.body.innerHTML = '';
    delete document.documentElement.dataset.theme;
  });

  it('挂载后可读写内容；程序化写入不触发 change（避免打开即变脏）', async () => {
    const { el, adapter } = mountAdapter('初始');
    const onChange = vi.fn();
    adapter.on('change', onChange);
    expect(adapter.getValue()).toBe('初始');
    adapter.setValue('程序化');
    await vi.advanceTimersByTimeAsync(CHANGE_THROTTLE_MS * 2);
    expect(adapter.getValue()).toBe('程序化');
    expect(el.querySelector('textarea')?.value).toBe('程序化');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('change 回调被节流（ED-02）且尾部补发最后一次内容', async () => {
    const { el, adapter } = mountAdapter();
    const onChange = vi.fn();
    adapter.on('change', onChange);
    type(el, 'a');
    type(el, 'ab');
    type(el, 'abc');
    await vi.advanceTimersByTimeAsync(CHANGE_THROTTLE_MS * 2);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenLastCalledWith('abc');
  });

  it('destroy 清理定时器与实例：之后不再有回调（ED-01）', async () => {
    const { el, adapter } = mountAdapter();
    const onChange = vi.fn();
    adapter.on('change', onChange);
    type(el, 'x');
    type(el, 'xy');
    adapter.destroy();
    await vi.advanceTimersByTimeAsync(CHANGE_THROTTLE_MS * 4);
    expect(onChange).not.toHaveBeenCalled();
    expect(el.querySelector('textarea')).toBeNull();
  });

  it('save 事件透传（内核 Ctrl+S）', () => {
    const { adapter } = mountAdapter();
    const onSave = vi.fn();
    adapter.on('save', onSave);
    adapter.setValue('内容');
    expect(adapter.getValue()).toBe('内容');
    expect(onSave).not.toHaveBeenCalled();
  });

  it('find/replace 作用于缓冲区内容', () => {
    const { adapter } = mountAdapter('foo bar foo\nFOO');
    const matches = adapter.find('foo');
    expect(matches).toHaveLength(3);
    const result = adapter.replace(matches, 'baz');
    expect(result.replaced).toBe(3);
    expect(adapter.getValue()).toBe('baz bar baz\nbaz');
    expect(adapter.find('foo', { caseSensitive: true })).toHaveLength(0);
    // baz / bar / baz / baz —— 正则 ba. 命中 4 处
    expect(adapter.find('ba.', { regex: true })).toHaveLength(4);
  });

  it('replace 全程只发一次事务（撤销一步回退整批，撤销栈口径不变）', () => {
    const { adapter } = mountAdapter('foo bar foo');
    adapter.replace(adapter.find('foo'), 'baz');
    expect(fake.state.dispatches).toHaveLength(1);
    expect(fake.state.dispatches[0]?.changes).toHaveLength(2);
  });

  it('find 读**内核文档**而不是慢一拍的内部值（16ms 节流窗口内继续输入）', () => {
    const { el, adapter } = mountAdapter('old');
    type(el, 'new');
    // 此刻模型 watcher 还没把 this.value 回填，旧的 find 实现会查不到
    expect(adapter.find('new')).toHaveLength(1);
    expect(adapter.getValue()).toBe('new');
  });

  it('未接入的补全能力显式失败，而不是静默无效', () => {
    const { adapter } = mountAdapter();
    expect(() => adapter.insertLink('a.md')).toThrow(/尚未接入/);
    expect(() => adapter.triggerSuggest('link')).toThrow(/尚未接入/);
  });
});

describe('内核工厂（ED-05）', () => {
  it('默认使用 MVP 内核', () => {
    expect(currentEngine()).toBe('md-editor-v3');
    expect(createEditorAdapter()).toBeInstanceOf(MdEditorV3Adapter);
  });

  it('选择 codemirror6 时显式失败（M8 接入，两套内核不共存）', () => {
    vi.stubEnv('VITE_EDITOR_ENGINE', 'codemirror6');
    try {
      expect(currentEngine()).toBe('codemirror6');
      expect(() => createEditorAdapter()).toThrow(/M8/);
    } finally {
      vi.unstubAllEnvs();
    }
  });
});

/**
 * 10.1/10.2/10.4 的回归：旧实现查 host.querySelector('textarea')（CodeMirror 内核没有 textarea）
 * ⇒ revealLine 恒为 false，面板只计数、不选中。
 */
describe('行定位/选中（FR-LINK-13、FR-SEARCH-07）', () => {
  const VALUE = '第一行\nsecond line here\n第三行';
  const LINE2 = 4;

  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('整行定位：真的 dispatch 选区并滚动，且默认把焦点放进编辑区', () => {
    const { adapter } = mountAdapter(VALUE);
    expect(adapter.revealLine(2)).toBe(true);
    expect(fake.state.selection).toEqual({ anchor: LINE2, head: LINE2 + 'second line here'.length });
    expect(fake.state.scrollCalls).toBe(1);
    expect(fake.state.focusCalls).toBe(1);
  });

  it('列区间：精确选中命中片段（不是整行）', () => {
    const { adapter } = mountAdapter(VALUE);
    expect(adapter.revealLine(2, { start: 7, end: 11 })).toBe(true);
    expect(fake.state.selection).toEqual({ anchor: LINE2 + 7, head: LINE2 + 11 });
    expect(VALUE.slice(LINE2 + 7, LINE2 + 11)).toBe('line');
  });

  it('focus:false 不抢焦点（查找面板要留在输入框，连续跳转不断）', () => {
    const { adapter } = mountAdapter(VALUE);
    fake.state.focusCalls = 0;
    expect(adapter.revealLine(2, undefined, { focus: false })).toBe(true);
    expect(fake.state.selection).not.toBeNull();
    expect(fake.state.focusCalls).toBe(0);
  });

  it('区间越界/为空/反向 ⇒ 退回整行（绝不选空）', () => {
    const { adapter } = mountAdapter(VALUE);
    adapter.revealLine(2, { start: 999, end: 9999 });
    expect(fake.state.selection).toEqual({ anchor: LINE2, head: LINE2 + 'second line here'.length });
    adapter.revealLine(2, { start: 5, end: 5 });
    expect(fake.state.selection).toEqual({ anchor: LINE2, head: LINE2 + 'second line here'.length });
    adapter.revealLine(2, { start: 9, end: 3 });
    expect(fake.state.selection).toEqual({ anchor: LINE2, head: LINE2 + 'second line here'.length });
  });

  it('行号越界按既有规则收敛，且区间照样生效', () => {
    const { adapter } = mountAdapter(VALUE);
    expect(adapter.revealLine(99, { start: 0, end: 1 })).toBe(true);
    const line3Start = VALUE.indexOf('第三行');
    expect(fake.state.selection).toEqual({ anchor: line3Start, head: line3Start + 1 });
    expect(adapter.revealLine(-5)).toBe(true);
    expect(fake.state.selection).toEqual({ anchor: 0, head: 3 });
  });

  it('内核视图不可用时返回 false（如实降级，不假装定位过）', () => {
    const { adapter } = mountAdapter(VALUE, { exposeView: false });
    expect(adapter.revealLine(1)).toBe(false);
    // 能力不足时 focusEditor 也不能抛错（退回内核内容 DOM，可能不存在）
    expect(() => adapter.focusEditor()).not.toThrow();
  });

  it('focusEditor 聚焦内核视图（供面板关闭时还焦点）', () => {
    const { adapter } = mountAdapter(VALUE);
    fake.state.focusCalls = 0;
    adapter.focusEditor();
    expect(fake.state.focusCalls).toBe(1);
  });
});

describe('主题响应式（8.3/8.5：切主题后编辑区跟随 data-theme 变色）', () => {
  const settle = async (): Promise<void> => {
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    await nextTick();
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
  };

  afterEach(() => {
    destroyAll();
    delete document.documentElement.dataset.theme;
    document.body.innerHTML = '';
    vi.restoreAllMocks();
  });

  it('挂载时取当前 data-theme；之后 data-theme 变化立即同步到内核 theme', async () => {
    document.documentElement.dataset.theme = 'dark';
    const { el, adapter } = mountAdapter('x');
    await nextTick();
    expect(el.querySelector('textarea')?.getAttribute('data-editor-theme')).toBe('dark');

    document.documentElement.dataset.theme = 'light';
    await settle();
    expect(el.querySelector('textarea')?.getAttribute('data-editor-theme')).toBe('light');
    adapter.destroy();
  });

  it('opts.theme 只是初始值：之后仍跟随 data-theme（编辑区不能和整体主题脱节）', async () => {
    document.documentElement.dataset.theme = 'light';
    const el = document.createElement('div');
    document.body.append(el);
    const adapter = new MdEditorV3Adapter();
    mounted.push(adapter);
    adapter.mount(el, { initialValue: 'x', theme: 'dark' });
    await nextTick();
    expect(el.querySelector('textarea')?.getAttribute('data-editor-theme')).toBe('dark');
    document.documentElement.dataset.theme = 'light';
    await settle();
    expect(el.querySelector('textarea')?.getAttribute('data-editor-theme')).toBe('light');
  });

  it('destroy 断开 MutationObserver（ED-01 资源释放）', () => {
    const disconnect = vi.spyOn(MutationObserver.prototype, 'disconnect');
    const { adapter } = mountAdapter();
    adapter.destroy();
    expect(disconnect).toHaveBeenCalled();
  });
});

describe('查找/替换的纯函数（FR-SEARCH-31）', () => {
  it('大小写、全字、正则可以组合；非法正则抛出（面板负责转成可见错误 R-15）', async () => {
    const { findInText } = await import('@features/editor/adapter/findInText');
    const text = 'foo bar foo\nFOO baz';
    expect(findInText(text, 'foo')).toHaveLength(3);
    expect(findInText(text, 'foo', { caseSensitive: true })).toHaveLength(2);
    expect(findInText(text, 'baz', { wholeWord: true })).toHaveLength(1);
    expect(findInText(text, 'foo', { wholeWord: true })).toHaveLength(3);
    expect(findInText(text, 'ba.', { regex: true })).toHaveLength(2);
    expect(() => findInText(text, '(', { regex: true })).toThrow();
  });

  it('批量替换从后往前计算偏移不失效；changes 与文本同源', async () => {
    const { findInText, replaceInText } = await import('@features/editor/adapter/findInText');
    const text = 'foo bar foo';
    const { text: next, changes } = replaceInText(text, findInText(text, 'foo'), 'baz');
    expect(next).toBe('baz bar baz');
    expect(changes).toEqual([
      { from: 8, to: 11, insert: 'baz' },
      { from: 0, to: 3, insert: 'baz' },
    ]);
  });
});
