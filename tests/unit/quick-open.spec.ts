import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const hoisted = vi.hoisted(() => ({ fulltext: vi.fn(), quick: vi.fn() }));
vi.mock('@core/ipc/searchCommands', () => ({
  searchFulltext: hoisted.fulltext,
  searchQuickOpen: hoisted.quick,
}));

import QuickOpen from '@features/search/components/QuickOpen.vue';
import { useSearchStore } from '@features/search/stores/search';

const items = [
  { relPath: 'work/report.md', name: 'report.md', score: 9 },
  { relPath: 'work/review/notes.md', name: 'notes.md', score: 7 },
  { relPath: 'archive/report-old.md', name: 'report-old.md', score: 3 },
];

async function typeInto(wrapper: ReturnType<typeof mount>, text: string): Promise<void> {
  await wrapper.find('[data-testid="quick-input"]').setValue(text);
  await vi.advanceTimersByTimeAsync(200);
  await flushPromises();
}

describe('快速打开（FR-SEARCH-40/41）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.useFakeTimers();
    hoisted.quick.mockReset();
    hoisted.quick.mockResolvedValue(items);
  });

  it('**匹配走后端**：输入路径片段后调用 search_quick_open（不在前端自己模糊匹配）', async () => {
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'work/rep');
    expect(hoisted.quick).toHaveBeenCalledTimes(1);
    expect(hoisted.quick.mock.calls[0]?.[0]).toBe('work/rep');
    expect(wrapper.findAll('[data-testid="quick-item"]')).toHaveLength(3);
  });

  it('输入防抖：连续输入只查最后一次', async () => {
    const wrapper = mount(QuickOpen);
    const input = wrapper.find('[data-testid="quick-input"]');
    await input.setValue('w');
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue('work');
    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();
    expect(hoisted.quick).toHaveBeenCalledTimes(1);
    expect(hoisted.quick.mock.calls[0]?.[0]).toBe('work');
  });

  it('默认选中第一条；↑/↓ 移动并夹紧两端', async () => {
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'rep');
    const store = useSearchStore();
    expect(store.quickSelected).toBe(0);
    const input = wrapper.find('[data-testid="quick-input"]');
    await input.trigger('keydown', { key: 'ArrowDown' });
    expect(store.quickSelected).toBe(1);
    await input.trigger('keydown', { key: 'ArrowDown' });
    await input.trigger('keydown', { key: 'ArrowDown' });
    expect(store.quickSelected).toBe(2); // 到底不环绕
    await input.trigger('keydown', { key: 'ArrowUp' });
    expect(store.quickSelected).toBe(1);
  });

  it('选中项在列表里有可见标记', async () => {
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'rep');
    await wrapper.find('[data-testid="quick-input"]').trigger('keydown', { key: 'ArrowDown' });
    const rows = wrapper.findAll('[data-testid="quick-item"]');
    expect(rows[1]?.classes()).toContain('kp-quick__item--selected');
    expect(rows[0]?.classes()).not.toContain('kp-quick__item--selected');
  });

  it('Enter ⇒ emit open(当前选中项)', async () => {
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'rep');
    const input = wrapper.find('[data-testid="quick-input"]');
    await input.trigger('keydown', { key: 'ArrowDown' });
    await input.trigger('keydown', { key: 'Enter' });
    expect(wrapper.emitted('open')?.[0]).toEqual(['work/review/notes.md']);
  });

  it('点击条目 ⇒ 直接打开那一条', async () => {
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'rep');
    await wrapper.findAll('[data-testid="quick-item"]')[2]?.trigger('click');
    expect(wrapper.emitted('open')?.[0]).toEqual(['archive/report-old.md']);
  });

  it('Esc 与点击空白处都关闭', async () => {
    const wrapper = mount(QuickOpen);
    await wrapper.find('[data-testid="quick-input"]').trigger('keydown', { key: 'Escape' });
    await wrapper.find('[data-testid="quick-open"]').trigger('click');
    expect(wrapper.emitted('close')).toHaveLength(2);
  });

  it('没有匹配 ⇒ 明确提示（不是空白）', async () => {
    hoisted.quick.mockResolvedValue([]);
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'zzz');
    expect(wrapper.find('[data-testid="quick-empty"]').exists()).toBe(true);
    expect(useSearchStore().quickSelected).toBe(-1);
  });

  it('查询失败 ⇒ 清空候选且不抛（不阻塞输入）', async () => {
    hoisted.quick.mockRejectedValue(new Error('boom'));
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'rep');
    expect(useSearchStore().quickItems).toHaveLength(0);
    expect(wrapper.find('[data-testid="quick-empty"]').exists()).toBe(true);
  });

  it('Enter 在没有候选时什么都不做', async () => {
    hoisted.quick.mockResolvedValue([]);
    const wrapper = mount(QuickOpen);
    await typeInto(wrapper, 'zzz');
    await wrapper.find('[data-testid="quick-input"]').trigger('keydown', { key: 'Enter' });
    expect(wrapper.emitted('open')).toBeUndefined();
  });

  it('暴露 focusInput 供 Mod+O 聚焦', async () => {
    const wrapper = mount(QuickOpen, { attachTo: document.body });
    (wrapper.vm as unknown as { focusInput: () => void }).focusInput();
    expect(document.activeElement).toBe(wrapper.find('[data-testid="quick-input"]').element);
    wrapper.unmount();
  });
});
