import { mount } from '@vue/test-utils';
import { describe, expect, it, vi } from 'vitest';
import FindReplacePanel from '@features/editor/components/FindReplacePanel.vue';
import { useFindPanel } from '@features/editor/composables/useFindPanel';

/**
 * 文件内查找替换面板（FR-SEARCH-30/31）。
 *
 * 10.1–10.4 的回归口径：面板只负责"算命中 + 请求定位"，**选中/标记由适配器落在内核选区上**；
 * 面板必须传 `focus:false`（否则每跳一处焦点就被抢走），关闭时把焦点交还编辑器。
 */
interface RevealCall {
  line: number;
  range: { start: number; end: number } | undefined;
  options: { focus?: boolean } | undefined;
}

function fakeAdapter(matches: { from: number; to: number; text: string }[], content = '') {
  const state = { text: content };
  const revealCalls: RevealCall[] = [];
  return {
    mount: vi.fn(),
    getValue: vi.fn(() => state.text),
    setValue: vi.fn(),
    on: vi.fn(),
    insertLink: vi.fn(),
    triggerSuggest: vi.fn(),
    find: vi.fn(() => matches),
    replace: vi.fn(() => ({ replaced: 1 })),
    // 记录三参调用：第三参（focus）是"焦点留在查找框"的回归口径
    revealLine: vi.fn((line: number, range?: { start: number; end: number }, options?: { focus?: boolean }) => {
      revealCalls.push({ line, range, options });
      return true;
    }),
    revealCalls,
    focusEditor: vi.fn(),
    destroy: vi.fn(),
  };
}

const CONTENT = 'foo bar foo\nbaz foo';
const MATCHES = [
  { from: 0, to: 3, text: 'foo' },
  { from: 8, to: 11, text: 'foo' },
  { from: 16, to: 19, text: 'foo' },
];
/** 面板给适配器的定位调用：第三参固定 focus:false（焦点留在查找框）。 */
const NO_FOCUS = { focus: false };

describe('FR-SEARCH-30/31 查找替换面板', () => {
  it('显示「第 X / 共 Y 处」，输入即定位到第一处', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'find' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 1 / 共 3 处');
    expect(adapter.revealLine).toHaveBeenCalledWith(1, { start: 0, end: 3 }, NO_FOCUS);
  });

  it('下一处/上一处可环绕，跳转定位并选中匹配段（10.2）', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'find' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    adapter.revealLine.mockClear();
    await w.get('[data-testid="find-next"]').trigger('click');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 2 / 共 3 处');
    expect(adapter.revealLine).toHaveBeenLastCalledWith(1, { start: 8, end: 11 }, NO_FOCUS);
    await w.get('[data-testid="find-prev"]').trigger('click');
    await w.get('[data-testid="find-prev"]').trigger('click');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 3 / 共 3 处');
    await w.get('[data-testid="find-next"]').trigger('click');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 1 / 共 3 处');
  });

  it('所有跳转都不抢焦点：next/prev/替换/重新查找一律 focus:false（10.1/10.2 的连续跳转）', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="find-replacement"]').setValue('bar');
    await w.get('[data-testid="find-next"]').trigger('click');
    await w.get('[data-testid="find-prev"]').trigger('click');
    await w.get('[data-testid="replace-one"]').trigger('click');
    expect(adapter.revealCalls.length).toBeGreaterThan(0);
    expect(adapter.revealCalls.every((call) => call.options?.focus === false)).toBe(true);
  });

  it('行/列换算用适配器的**当前文本**（content prop 可能慢一拍，10.4 的标记错位同源）', async () => {
    // 内核里是 "AAA\nfoo"（命中在第 2 行行首），但 content prop 还是旧的 "foo"
    const adapter = fakeAdapter([{ from: 4, to: 7, text: 'foo' }], 'AAA\nfoo');
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'find' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    expect(adapter.revealLine).toHaveBeenLastCalledWith(2, { start: 0, end: 3 }, NO_FOCUS);
  });

  it('Enter 下一处、Shift+Enter 上一处、Esc 请求关闭', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'find' } });
    const input = w.get('[data-testid="find-query"]');
    await input.setValue('foo');
    await input.trigger('keydown', { key: 'Enter' });
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 2 / 共 3 处');
    await input.trigger('keydown', { key: 'Enter', shiftKey: true });
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 1 / 共 3 处');
    await input.trigger('keydown', { key: 'Escape' });
    expect(w.emitted('close')).toHaveLength(1);
  });

  it('逐条替换只替换当前一条', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="find-replacement"]').setValue('bar');
    await w.get('[data-testid="replace-one"]').trigger('click');
    expect(adapter.replace).toHaveBeenCalledWith([{ from: 0, to: 3, text: 'foo' }], 'bar');
  });

  it('全部替换；**没有命中时绝不调用 replace**（不写任何东西）', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="find-replacement"]').setValue('bar');
    await w.get('[data-testid="replace-all"]').trigger('click');
    expect(adapter.replace).toHaveBeenCalledWith(MATCHES, 'bar');

    const empty = fakeAdapter([], CONTENT);
    const w2 = mount(FindReplacePanel, { props: { adapter: empty, mode: 'replace' } });
    await w2.get('[data-testid="find-query"]').setValue('zzz');
    await w2.get('[data-testid="replace-all"]').trigger('click');
    expect(empty.replace).not.toHaveBeenCalled();
  });

  it('替换失败可见（R-15），不静默', async () => {
    const adapter = fakeAdapter(MATCHES, CONTENT);
    adapter.replace.mockImplementationOnce(() => {
      throw { code: 'E_DB', message: '写入失败' };
    });
    const w = mount(FindReplacePanel, { props: { adapter, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="replace-all"]').trigger('click');
    expect(w.get('[data-testid="find-error"]').text()).toContain('写入失败');
  });
});

describe('useFindPanel（FR-SEARCH-30；10.3 关闭后焦点回编辑区）', () => {
  it('closeFind 关闭面板并调用 adapter.focusEditor（不是聚焦不可聚焦的外层 div）', () => {
    const adapter = { focusEditor: vi.fn() };
    const panel = useFindPanel(() => adapter);
    panel.openFind('find');
    expect(panel.findOpen.value).toBe(true);
    panel.closeFind();
    expect(panel.findOpen.value).toBe(false);
    expect(adapter.focusEditor).toHaveBeenCalledTimes(1);
  });

  it('适配器尚未挂载（null）时关闭不抛错', () => {
    const panel = useFindPanel(() => null);
    panel.openFind('replace');
    expect(panel.findMode.value).toBe('replace');
    expect(() => panel.closeFind()).not.toThrow();
    expect(panel.findOpen.value).toBe(false);
  });

  it('toggleFind：工具栏再次点击等于关闭，也要把焦点还回去', () => {
    const adapter = { focusEditor: vi.fn() };
    const panel = useFindPanel(() => adapter);
    panel.toggleFind();
    expect(panel.findOpen.value).toBe(true);
    panel.toggleFind();
    expect(panel.findOpen.value).toBe(false);
    expect(adapter.focusEditor).toHaveBeenCalledTimes(1);
  });
});