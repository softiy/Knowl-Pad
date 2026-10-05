import { defineComponent, h } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/** 用极简替身替换 md-editor-v3：只测我们这一侧的适配逻辑（jsdom 跑不动真实内核）。 */
vi.mock('md-editor-v3', () => ({
  MdEditor: defineComponent({
    name: 'MdEditorStub',
    props: { modelValue: { type: String, default: '' } },
    emits: ['update:modelValue', 'onSave'],
    setup(props, { emit }) {
      return () =>
        h('textarea', {
          class: 'stub-editor',
          value: props.modelValue,
          onInput: (event: Event) => {
            emit('update:modelValue', (event.target as HTMLTextAreaElement).value);
          },
        });
    },
  }),
}));

import { CHANGE_THROTTLE_MS, MdEditorV3Adapter } from '@features/editor';
import { createEditorAdapter, currentEngine } from '@features/editor/adapter';

function mountAdapter(initial = '') {
  const el = document.createElement('div');
  document.body.append(el);
  const adapter = new MdEditorV3Adapter();
  adapter.mount(el, { initialValue: initial });
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
    vi.useRealTimers();
    document.body.innerHTML = '';
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
