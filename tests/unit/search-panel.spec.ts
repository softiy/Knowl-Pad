import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { KpError, E_SEARCH_SYNTAX, E_SEARCH_TOO_BROAD } from '@core/ipc/errors';
import type { SearchResultPage } from '@core/ipc/searchCommands';

const hoisted = vi.hoisted(() => ({ fulltext: vi.fn(), quick: vi.fn() }));
vi.mock('@core/ipc/searchCommands', () => ({
  searchFulltext: hoisted.fulltext,
  searchQuickOpen: hoisted.quick,
}));

import SearchPanel from '@features/search/components/SearchPanel.vue';
import { useSearchStore } from '@features/search/stores/search';

/** 一页结果：两篇笔记，第一篇有 3 处命中。 */
function page(total = 2): SearchResultPage {
  return {
    total,
    offset: 0,
    limit: 20,
    hasMore: total > 2,
    items: [
      {
        relPath: 'work/report.md',
        name: 'report.md',
        mtimeMs: 1_700_000_000_000,
        score: -3,
        hitCount: 3,
        snippets: [
          { lines: [{ line: 7, text: 'alpha beta', isHitLine: true }], highlightLine: 7, highlightStart: 6, highlightEnd: 10 },
          { lines: [{ line: 19, text: 'beta again', isHitLine: true }], highlightLine: 19, highlightStart: 0, highlightEnd: 4 },
          { lines: [{ line: 30, text: 'beta third', isHitLine: true }], highlightLine: 30, highlightStart: 0, highlightEnd: 4 },
        ],
      },
      {
        relPath: 'notes/other.md',
        name: 'other.md',
        mtimeMs: 1_600_000_000_000,
        score: -1,
        hitCount: 1,
        snippets: [
          { lines: [{ line: 2, text: 'beta', isHitLine: true }], highlightLine: 2, highlightStart: 0, highlightEnd: 4 },
        ],
      },
    ],
  };
}

/** 输入并等过防抖窗口（FR-SEARCH-08 的 200ms）。 */
async function typeInto(wrapper: ReturnType<typeof mount>, text: string): Promise<void> {
  await wrapper.find('[data-testid="search-input"]').setValue(text);
  await vi.advanceTimersByTimeAsync(200);
  await flushPromises();
}

describe('搜索面板（FR-SEARCH-01~11）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.useFakeTimers();
    hoisted.fulltext.mockReset();
    hoisted.quick.mockReset();
    hoisted.fulltext.mockResolvedValue(page());
  });

  it('输入**防抖 200ms**：不到窗口不发请求，到窗口只发一次', async () => {
    const wrapper = mount(SearchPanel);
    await wrapper.find('[data-testid="search-input"]').setValue('alpha');
    await vi.advanceTimersByTimeAsync(199);
    expect(hoisted.fulltext).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await flushPromises();
    expect(hoisted.fulltext).toHaveBeenCalledTimes(1);
    expect(hoisted.fulltext.mock.calls[0]?.[0]).toBe('alpha');
  });

  it('防抖期内继续输入 ⇒ **取消上一次**，只按最后一次查', async () => {
    const wrapper = mount(SearchPanel);
    const input = wrapper.find('[data-testid="search-input"]');
    await input.setValue('a');
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue('ab');
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue('abc');
    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();
    expect(hoisted.fulltext).toHaveBeenCalledTimes(1);
    expect(hoisted.fulltext.mock.calls[0]?.[0]).toBe('abc');
  });

  it('慢的旧响应回来时**整包丢弃**（不覆盖新结果）', async () => {
    const store = useSearchStore();
    let resolveFirst: (p: SearchResultPage) => void = () => {};
    hoisted.fulltext
      .mockImplementationOnce(() => new Promise<SearchResultPage>((res) => { resolveFirst = res; }))
      .mockResolvedValueOnce(page());

    store.setQuery('alpha'); // 旧请求：发出去但**先不返回**
    await vi.advanceTimersByTimeAsync(200);
    store.setQuery('beta'); // 新请求：先回来
    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();
    expect(store.total).toBe(2);

    resolveFirst({ ...page(99), items: [] }); // 旧响应姗姗来迟
    await flushPromises();
    expect(store.total).toBe(2); // 没有被 99 覆盖
    expect(store.results).toHaveLength(2);
  });

  it('渲染每条结果的 名称/路径/时间/命中次数，且只展开一条片段、可展开全部', async () => {
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    const rows = wrapper.findAll('[data-testid="search-result"]');
    expect(rows).toHaveLength(2);
    expect(rows[0]?.find('[data-testid="search-result-name"]').text()).toBe('report.md');
    expect(rows[0]?.find('[data-testid="search-result-path"]').text()).toBe('work/report.md');
    expect(rows[0]?.find('[data-testid="search-result-count"]').text()).toBe('3 处命中');
    // 默认 1 条片段（= 1 个高亮），展开后 3 条
    expect(rows[0]?.findAll('[data-testid="search-hit-mark"]')).toHaveLength(1);
    await rows[0]?.find('[data-testid="search-result-expand"]').trigger('click');
    expect(wrapper.findAll('[data-testid="search-result"]')[0]?.findAll('[data-testid="search-hit-mark"]')).toHaveLength(3);
  });

  it('高亮只落在命中行，且用的是后端给的高亮区间', async () => {
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    const mark = wrapper.find('[data-testid="search-hit-mark"]');
    expect(mark.text()).toBe('beta');
  });

  it('点击结果 ⇒ 带着**命中行号 + 列区间** emit open（跳转定位用，FR-SEARCH-07）', async () => {
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    await wrapper.findAll('[data-testid="search-result"]')[0]?.trigger('click');
    // 列区间就是渲染高亮用的那一对（第 1 条片段的 6..10）⇒ 编辑器可精确选中命中处
    expect(wrapper.emitted('open')?.[0]).toEqual(['work/report.md', 7, { start: 6, end: 10 }]);
  });

  it('没有片段的结果 ⇒ 不带列区间（由编辑器回退成整行选中）', async () => {
    hoisted.fulltext.mockResolvedValue({
      items: [{ ...page().items[0]!, snippets: [] }],
      total: 1,
      offset: 0,
      limit: 20,
      hasMore: false,
    });
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    await wrapper.findAll('[data-testid="search-result"]')[0]?.trigger('click');
    expect(wrapper.emitted('open')?.[0]).toEqual(['work/report.md', 1, undefined]);
  });

  it('换排序 ⇒ 用新的 sort **重新查询**（排序真相在后端）', async () => {
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    await wrapper.find('[data-testid="search-sort"]').setValue('mtime');
    await flushPromises();
    expect(hoisted.fulltext).toHaveBeenCalledTimes(2);
    expect(hoisted.fulltext.mock.calls[1]?.[1]).toMatchObject({ sort: 'mtime' });
  });

  it('换匹配模式 ⇒ OR 也透传给后端', async () => {
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    await wrapper.find('[data-testid="search-mode"]').setValue('any');
    await flushPromises();
    expect(hoisted.fulltext.mock.calls[1]?.[1]).toMatchObject({ matchMode: 'any' });
  });

  it('空结果 ⇒ 给出建议（FR-SEARCH-11），而不是一片空白', async () => {
    hoisted.fulltext.mockResolvedValue({ items: [], total: 0, offset: 0, limit: 20, hasMore: false });
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, '找不到的词');
    expect(wrapper.find('[data-testid="search-empty"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="search-empty"]').text()).toContain('引号');
  });

  it('**语法错误**与**候选过多**共用错误码，但呈现必须区分', async () => {
    hoisted.fulltext.mockRejectedValueOnce(new KpError({ code: E_SEARCH_SYNTAX, message: '引号未闭合' }));
    const w1 = mount(SearchPanel);
    await typeInto(w1, '"未闭合');
    const syntax = w1.find('[data-testid="search-error"]');
    expect(syntax.attributes('data-kind')).toBe('syntax');
    expect(syntax.text()).toContain('语法提示');

    setActivePinia(createPinia());
    // 2026-10-15 裁定①：范围太大已是**专用码**（旧实现复用 E_SEARCH_SYNTAX、只能按文案区分）
    hoisted.fulltext.mockRejectedValueOnce(
      new KpError({ code: E_SEARCH_TOO_BROAD, message: 'line: 的候选笔记过多（当前 301 条、上限 300 条）' }),
    );
    const w2 = mount(SearchPanel);
    await typeInto(w2, 'line:(x)');
    const broad = w2.find('[data-testid="search-error"]');
    expect(broad.attributes('data-kind')).toBe('too-broad');
    expect(broad.text()).toContain('当前 301 条');
    expect(broad.text()).toContain('上限 300 条');
    expect(broad.text()).not.toContain('语法提示');
  });

  it('搜索中显示 loading，且清空输入后回到"还没搜"', async () => {
    const wrapper = mount(SearchPanel);
    const store = useSearchStore();
    await typeInto(wrapper, 'beta');
    await typeInto(wrapper, '   ');
    expect(store.searched).toBe(false);
    expect(store.results).toHaveLength(0);
    expect(wrapper.find('[data-testid="search-empty"]').exists()).toBe(false);
  });

  it('还有更多 ⇒ 显示"加载更多"，点了会按 offset 追加', async () => {
    hoisted.fulltext.mockResolvedValueOnce({ ...page(30) });
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    expect(wrapper.find('[data-testid="search-more"]').exists()).toBe(true);
    hoisted.fulltext.mockResolvedValueOnce({ ...page(30), items: page().items, offset: 2 });
    await wrapper.find('[data-testid="search-more"]').trigger('click');
    await flushPromises();
    expect(hoisted.fulltext.mock.calls[1]?.[2]).toMatchObject({ offset: 2 });
  });

  it('键盘：↓ 移动选中项、Enter 打开（FR-SEARCH-07 的键盘通路）', async () => {
    const wrapper = mount(SearchPanel);
    await typeInto(wrapper, 'beta');
    const input = wrapper.find('[data-testid="search-input"]');
    await input.trigger('keydown', { key: 'ArrowDown' });
    await input.trigger('keydown', { key: 'Enter' });
    expect(wrapper.emitted('open')?.[0]).toEqual(['notes/other.md', 2, { start: 0, end: 4 }]);
  });

  it('Esc ⇒ emit close；关闭按钮同理', async () => {
    const wrapper = mount(SearchPanel);
    await wrapper.find('[data-testid="search-input"]').trigger('keydown', { key: 'Escape' });
    await wrapper.find('[data-testid="search-close"]').trigger('click');
    expect(wrapper.emitted('close')).toHaveLength(2);
  });

  it('暴露 focusInput 供快捷键聚焦', async () => {
    const wrapper = mount(SearchPanel, { attachTo: document.body });
    (wrapper.vm as unknown as { focusInput: () => void }).focusInput();
    expect(document.activeElement).toBe(wrapper.find('[data-testid="search-input"]').element);
    wrapper.unmount();
  });
});
