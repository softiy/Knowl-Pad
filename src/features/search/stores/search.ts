import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import {
  searchFulltext,
  searchQuickOpen,
  type QuickOpenItem,
  type SearchResultItem,
} from '@core/ipc/searchCommands';
import {
  SEARCH_DEBOUNCE_MS,
  classifySearchError,
  createLatestGuard,
  nextIndex,
  visibleSnippets,
  type ClassifiedError,
  type SearchSort,
} from '../logic';

/** 每页条数（与后端缺省一致：IPC-05 的载荷控制）。 */
export const PAGE_SIZE = 20;

/**
 * 搜索面板与快速打开的数据源（FR-SEARCH-01~11 / 40 / 41）。
 *
 * 三条口径（都不是随手定的）：
 * 1. **结果合并与排序都在后端**：`hitCount` 与 `snippets` 是后端算好的，UI 不做二次归并 ——
 *    否则"同一篇算几条"会出现两个真相；排序切换也是改 `options.sort` **重新查询**。
 * 2. **取消 = 丢弃过期响应**（FR-SEARCH-08）：IPC 无法 abort，所以用 `createLatestGuard`
 *    给每次请求编号，回来时不是最新号就整包丢掉。
 * 3. **导航不在本 store**：面板只 `emit('open', { relPath, line })`，由 WorkspaceLayout 交给
 *    `editor.reveal`（与反链面板同一条既有通路，不另造一套定位机制）。
 */
export const useSearchStore = defineStore('search', () => {
  const panelOpen = ref(false);
  const quickOpenVisible = ref(false);

  const query = ref('');
  const sort = ref<SearchSort>('relevance');
  const matchMode = ref<'all' | 'any'>('all');
  const results = ref<SearchResultItem[]>([]);
  const total = ref(0);
  const loading = ref(false);
  /** 是否已经跑过一次查询：用来区分"还没搜"与"搜了但没结果"。 */
  const searched = ref(false);
  const error = ref<ClassifiedError | null>(null);
  const expanded = ref<string[]>([]);
  const selected = ref(-1);

  const quickQuery = ref('');
  const quickItems = ref<QuickOpenItem[]>([]);
  const quickSelected = ref(-1);

  const fulltextGuard = createLatestGuard();
  const quickGuard = createLatestGuard();
  let fulltextTimer: ReturnType<typeof setTimeout> | null = null;
  let quickTimer: ReturnType<typeof setTimeout> | null = null;

  const isEmpty = computed(() => searched.value && !loading.value && results.value.length === 0);
  const hasMore = computed(() => results.value.length < total.value);

  /** 命中行号（1 基，与索引库同口径）；没有片段时退化为第 1 行。 */
  function hitLineFor(item: SearchResultItem): number {
    return item.snippets[0]?.highlightLine ?? 1;
  }

  function snippetsOf(item: SearchResultItem) {
    return visibleSnippets(item, expanded.value.includes(item.relPath));
  }

  function toggleExpand(relPath: string): void {
    const i = expanded.value.indexOf(relPath);
    if (i >= 0) expanded.value.splice(i, 1);
    else expanded.value.push(relPath);
  }

  async function runFulltext(offset: number): Promise<void> {
    const q = query.value.trim();
    const id = fulltextGuard.next();
    if (!q) {
      results.value = [];
      total.value = 0;
      searched.value = false;
      error.value = null;
      return;
    }
    loading.value = true;
    error.value = null;
    try {
      const page = await searchFulltext(
        q,
        { sort: sort.value, matchMode: matchMode.value, recordHistory: false },
        { offset, limit: PAGE_SIZE },
      );
      if (!fulltextGuard.isCurrent(id)) return; // 过期响应：整包丢弃（FR-SEARCH-08）
      results.value = offset === 0 ? page.items : [...results.value, ...page.items];
      total.value = page.total;
      searched.value = true;
      selected.value = results.value.length > 0 ? 0 : -1;
    } catch (err) {
      if (!fulltextGuard.isCurrent(id)) return;
      error.value = classifySearchError(err);
      results.value = [];
      total.value = 0;
      searched.value = true;
    } finally {
      if (fulltextGuard.isCurrent(id)) loading.value = false;
    }
  }

  /** 输入变化：**防抖 200ms**（FR-SEARCH-08 前半），并把上一次的定时器取消掉。 */
  function scheduleSearch(): void {
    if (fulltextTimer !== null) clearTimeout(fulltextTimer);
    fulltextTimer = setTimeout(() => {
      fulltextTimer = null;
      void runFulltext(0);
    }, SEARCH_DEBOUNCE_MS);
  }

  function setQuery(value: string): void {
    query.value = value;
    scheduleSearch();
  }

  /** 换排序/匹配模式：立即重查（用户按的是按钮，不需要再等防抖）。 */
  function setSort(value: SearchSort): void {
    sort.value = value;
    void runFulltext(0);
  }

  function setMatchMode(value: 'all' | 'any'): void {
    matchMode.value = value;
    void runFulltext(0);
  }

  function loadMore(): void {
    if (!loading.value && hasMore.value) void runFulltext(results.value.length);
  }

  function moveSelection(delta: number): void {
    selected.value = nextIndex(selected.value, results.value.length, delta);
  }

  async function runQuick(): Promise<void> {
    const q = quickQuery.value.trim();
    const id = quickGuard.next();
    try {
      const items = await searchQuickOpen(q, 20);
      if (!quickGuard.isCurrent(id)) return;
      quickItems.value = items;
      quickSelected.value = items.length > 0 ? 0 : -1;
    } catch {
      if (!quickGuard.isCurrent(id)) return;
      quickItems.value = [];
      quickSelected.value = -1;
    }
  }

  function setQuickQuery(value: string): void {
    quickQuery.value = value;
    if (quickTimer !== null) clearTimeout(quickTimer);
    quickTimer = setTimeout(() => {
      quickTimer = null;
      void runQuick();
    }, SEARCH_DEBOUNCE_MS);
  }

  function moveQuickSelection(delta: number): void {
    quickSelected.value = nextIndex(quickSelected.value, quickItems.value.length, delta);
  }

  function openPanel(): void {
    panelOpen.value = true;
    quickOpenVisible.value = false;
  }

  function closePanel(): void {
    panelOpen.value = false;
  }

  function openQuickOpen(): void {
    quickOpenVisible.value = true;
    panelOpen.value = false;
    void runQuick();
  }

  function closeQuickOpen(): void {
    quickOpenVisible.value = false;
  }

  /** 选中项（面板 / 快速打开），供 Enter 与点击共用。 */
  function selectedItem(): SearchResultItem | null {
    return results.value[selected.value] ?? null;
  }

  function selectedQuick(): QuickOpenItem | null {
    return quickItems.value[quickSelected.value] ?? null;
  }

  return {
    panelOpen, quickOpenVisible, query, sort, matchMode, results, total, loading,
    searched, error, expanded, selected, quickQuery, quickItems, quickSelected,
    isEmpty, hasMore,
    hitLineFor, snippetsOf, toggleExpand, setQuery, setSort, setMatchMode,
    loadMore, moveSelection, runFulltext, setQuickQuery, moveQuickSelection,
    openPanel, closePanel, openQuickOpen, closeQuickOpen, runQuick,
    selectedItem, selectedQuick,
  };
});
