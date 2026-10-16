import { flushPromises } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import type { SearchResultPage } from '@core/ipc/searchCommands';

const hoisted = vi.hoisted(() => ({ fulltext: vi.fn(), quick: vi.fn() }));
vi.mock('@core/ipc/searchCommands', () => ({
  searchFulltext: hoisted.fulltext,
  searchQuickOpen: hoisted.quick,
}));

import { useSearchStore } from '@features/search/stores/search';

const emptyPage: SearchResultPage = { items: [], total: 0, offset: 0, limit: 20, hasMore: false };

/** 带 n 条片段的结果（用于 snippetsOf / hitLineFor 的边界）。 */
function itemWith(snippets: number, relPath = 'a.md') {
  return {
    relPath,
    name: relPath,
    mtimeMs: 1,
    score: -1,
    hitCount: snippets,
    snippets: Array.from({ length: snippets }, (_, i) => ({
      lines: [{ line: i + 1, text: 't', isHitLine: true }],
      highlightLine: i + 1,
      highlightStart: 0,
      highlightEnd: 1,
    })),
  };
}

describe('搜索 store：入口开关与派生（FR-SEARCH-01/40）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.fulltext.mockReset();
    hoisted.quick.mockReset();
    hoisted.quick.mockResolvedValue([]);
  });

  it('打开面板/快速打开是**互斥**的两块界面', () => {
    const s = useSearchStore();
    s.openQuickOpen();
    expect(s.quickOpenVisible).toBe(true);
    expect(s.panelOpen).toBe(false);
    s.openPanel();
    expect(s.panelOpen).toBe(true);
    expect(s.quickOpenVisible).toBe(false);
    s.closePanel();
    s.closeQuickOpen();
    expect(s.panelOpen).toBe(false);
    expect(s.quickOpenVisible).toBe(false);
  });

  it('快速打开打开时会立刻查一次（空查询 = 列全部候选）', async () => {
    const s = useSearchStore();
    s.openQuickOpen();
    await flushPromises();
    expect(hoisted.quick).toHaveBeenCalledTimes(1);
  });

  it('没有选中项时返回 null（Enter 不该打开任何东西）', () => {
    const s = useSearchStore();
    expect(s.selectedItem()).toBeNull();
    expect(s.selectedQuick()).toBeNull();
  });

  it('snippetsOf 跟随展开状态；hitLineFor 在没有片段时退化为第 1 行', () => {
    const s = useSearchStore();
    const multi = itemWith(3);
    expect(s.snippetsOf(multi)).toHaveLength(1);
    s.toggleExpand('a.md');
    expect(s.snippetsOf(multi)).toHaveLength(3);
    expect(s.hitLineFor(multi)).toBe(1);
    expect(s.hitLineFor(itemWith(0))).toBe(1);
    s.toggleExpand('a.md'); // 再点一次是收起
    expect(s.snippetsOf(multi)).toHaveLength(1);
  });

  it('空结果时 hasMore=false，loadMore 不该再发请求', async () => {
    hoisted.fulltext.mockResolvedValue(emptyPage);
    const s = useSearchStore();
    s.setQuery('x');
    await vi.waitFor(() => expect(s.searched).toBe(true));
    const before = hoisted.fulltext.mock.calls.length;
    s.loadMore();
    await flushPromises();
    expect(hoisted.fulltext.mock.calls.length).toBe(before);
  });

  it('查询为空 ⇒ 立刻清空且不发请求（不等防抖）', async () => {
    const s = useSearchStore();
    s.setQuery('   ');
    await flushPromises();
    expect(hoisted.fulltext).not.toHaveBeenCalled();
    expect(s.results).toHaveLength(0);
    expect(s.total).toBe(0);
    expect(s.error).toBeNull();
  });

  it('非语法错误 ⇒ other（不误导成语法问题）', async () => {
    hoisted.fulltext.mockRejectedValue(new Error('索引库损坏'));
    const s = useSearchStore();
    s.setQuery('x');
    await vi.waitFor(() => expect(s.error).not.toBeNull());
    expect(s.error?.kind).toBe('other');
    expect(s.error?.title).toBe('搜索失败');
  });

  it('moveSelection 在空列表上停在 -1', () => {
    const s = useSearchStore();
    s.moveSelection(1);
    expect(s.selected).toBe(-1);
    s.moveQuickSelection(-1);
    expect(s.quickSelected).toBe(-1);
  });

  it('查询为空时换排序不发请求（没有查询词就没有可排的东西），但设置要生效', async () => {
    hoisted.fulltext.mockResolvedValue(emptyPage);
    const s = useSearchStore();
    s.setSort('path');
    await flushPromises();
    expect(hoisted.fulltext).not.toHaveBeenCalled();
    expect(s.sort).toBe('path');
  });

  it('正在加载时再点"加载更多"**不会并发重入**', async () => {
    hoisted.fulltext.mockResolvedValueOnce({
      items: [itemWith(1)],
      total: 50,
      offset: 0,
      limit: 20,
      hasMore: true,
    });
    const s = useSearchStore();
    s.setQuery('x');
    await vi.waitFor(() => expect(s.results).toHaveLength(1));

    let release: (p: SearchResultPage) => void = () => {};
    hoisted.fulltext.mockImplementationOnce(
      () => new Promise<SearchResultPage>((res) => {
        release = res;
      }),
    );
    s.loadMore(); // 进入 loading
    s.loadMore(); // 应被 guard 拦下
    expect(hoisted.fulltext).toHaveBeenCalledTimes(2);
    release({ items: [itemWith(1, 'b.md')], total: 50, offset: 1, limit: 20, hasMore: true });
    await flushPromises();
    expect(s.results).toHaveLength(2); // 追加而不是覆盖
    expect(hoisted.fulltext).toHaveBeenCalledTimes(2);
  });
});
