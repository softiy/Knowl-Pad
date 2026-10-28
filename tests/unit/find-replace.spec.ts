import { mount } from '@vue/test-utils';
import { describe, expect, it, vi } from 'vitest';
import FindReplacePanel from '@features/editor/components/FindReplacePanel.vue';

function fakeAdapter(matches: { from: number; to: number; text: string }[]) {
  return {
    mount: vi.fn(), getValue: vi.fn(() => ''), setValue: vi.fn(), on: vi.fn(),
    insertLink: vi.fn(), triggerSuggest: vi.fn(),
    find: vi.fn(() => matches),
    replace: vi.fn(() => ({ replaced: 1 })),
    revealLine: vi.fn(() => true), destroy: vi.fn(),
  };
}

const CONTENT = 'foo bar foo\nbaz foo';
const MATCHES = [
  { from: 0, to: 3, text: 'foo' },
  { from: 8, to: 11, text: 'foo' },
  { from: 16, to: 19, text: 'foo' },
];

describe('FR-SEARCH-30/31 查找替换面板', () => {
  it('显示「第 X / 共 Y 处」，输入即定位到第一处', async () => {
    const adapter = fakeAdapter(MATCHES);
    const w = mount(FindReplacePanel, { props: { adapter, content: CONTENT, mode: 'find' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 1 / 共 3 处');
    expect(adapter.revealLine).toHaveBeenCalledWith(1, { start: 0, end: 3 });
  });

  it('下一处/上一处可环绕，跳转定位并选中匹配段', async () => {
    const adapter = fakeAdapter(MATCHES);
    const w = mount(FindReplacePanel, { props: { adapter, content: CONTENT, mode: 'find' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    adapter.revealLine.mockClear();
    await w.get('[data-testid="find-next"]').trigger('click');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 2 / 共 3 处');
    expect(adapter.revealLine).toHaveBeenLastCalledWith(1, { start: 8, end: 11 });
    await w.get('[data-testid="find-prev"]').trigger('click');
    await w.get('[data-testid="find-prev"]').trigger('click');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 3 / 共 3 处');
    await w.get('[data-testid="find-next"]').trigger('click');
    expect(w.get('[data-testid="find-count"]').text()).toBe('第 1 / 共 3 处');
  });

  it('Enter 下一处、Shift+Enter 上一处、Esc 请求关闭', async () => {
    const adapter = fakeAdapter(MATCHES);
    const w = mount(FindReplacePanel, { props: { adapter, content: CONTENT, mode: 'find' } });
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
    const adapter = fakeAdapter(MATCHES);
    const w = mount(FindReplacePanel, { props: { adapter, content: CONTENT, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="find-replacement"]').setValue('bar');
    await w.get('[data-testid="replace-one"]').trigger('click');
    expect(adapter.replace).toHaveBeenCalledWith([{ from: 0, to: 3, text: 'foo' }], 'bar');
  });

  it('全部替换；**没有命中时绝不调用 replace**（不写任何东西）', async () => {
    const adapter = fakeAdapter(MATCHES);
    const w = mount(FindReplacePanel, { props: { adapter, content: CONTENT, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="find-replacement"]').setValue('bar');
    await w.get('[data-testid="replace-all"]').trigger('click');
    expect(adapter.replace).toHaveBeenCalledWith(MATCHES, 'bar');

    const empty = fakeAdapter([]);
    const w2 = mount(FindReplacePanel, { props: { adapter: empty, content: CONTENT, mode: 'replace' } });
    await w2.get('[data-testid="find-query"]').setValue('zzz');
    await w2.get('[data-testid="replace-all"]').trigger('click');
    expect(empty.replace).not.toHaveBeenCalled();
  });

  it('替换失败可见（R-15），不静默', async () => {
    const adapter = fakeAdapter(MATCHES);
    adapter.replace.mockImplementationOnce(() => { throw { code: 'E_DB', message: '写入失败' }; });
    const w = mount(FindReplacePanel, { props: { adapter, content: CONTENT, mode: 'replace' } });
    await w.get('[data-testid="find-query"]').setValue('foo');
    await w.get('[data-testid="replace-all"]').trigger('click');
    expect(w.get('[data-testid="find-error"]').text()).toContain('写入失败');
  });
});
