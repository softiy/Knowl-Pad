import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import { onFsCreated, onFsModified, onFsRemoved, onFsRenamed, onIndexCompleted } from '@core/ipc/events';
import { tagNotes, tagTree, type TaggedNote, type TagNode } from '@core/ipc/tagCommands';
import { asKpError } from '@core/ipc/errors';
import {
  DEFAULT_TAG_SORT,
  filterTree,
  findTagNode,
  flattenTree,
  sortTree,
  walkTree,
  type TagRow,
  type TagSortMode,
} from '../logic';

/** 变更后合并刷新的窗口（同链接面板口径：远小于 FR-LINK-17 的「1 秒内」）。 */
const REFRESH_DEBOUNCE_MS = 250;

/**
 * 正文标签名 → 索引里的规范键（`tag.norm`）：小写 + **折叠空段**（`/a//b/` → `a/b`）。
 *
 * 与 Rust 的 `md_parse::tag::clean` + `norm` 同口径 —— 口径不一致会点出一个查不到的标签（点进去空列表）。
 */
export function normalizeTagName(raw: string): string {
  return raw
    .trim()
    .split('/')
    .filter((seg) => seg !== '')
    .join('/')
    .toLowerCase();
}

/**
 * 标签面板的数据源（FR-TAG-01~04 / 13）。
 *
 * 三条口径（都不是随手定的）：
 * 1. **树与计数都来自索引库实时查询**（FR-TAG-12 要求 `ref_count` 与 `file_tag` 实际计数一致）：
 *    这里不做本地累加 —— 祖先的 refCount **天然已含全部子孙**（MD-TAG-03 的祖先登记），
 *    再逐级相加就是 PRD §5.3.6.1 点名的重复计数。
 * 2. **"只认最后一次请求"**：连点标签时 IPC 无序返回，慢的旧响应不能盖掉新的（同搜索域）。
 * 3. **导航不在本 store**：面板只 `emit('open')`，由 WorkspaceLayout 交给既有通路。
 */
export const useTagsStore = defineStore('tags', () => {
  /** 原始树（未排序、未过滤）：排序/过滤是纯函数，放在派生层做，原始数据只有一份。 */
  const tree = ref<TagNode[]>([]);
  const sortMode = ref<TagSortMode>(DEFAULT_TAG_SORT);
  const filter = ref('');
  /** 当前选中的标签规范键（`null` = 未选；`''` = 「未分类」，是**合法选中**）。 */
  const selectedNorm = ref<string | null>(null);
  const notes = ref<TaggedNote[]>([]);
  const notesLoading = ref(false);
  /**
   * FR-TAG-03 的「是否含子级」。**自 2026-10-17（批次 4）起真正生效**（勘误 D-40②）：
   * `false` ⇒ 只列**显式写了该标签**的笔记（`file_tag.explicit = 1`）；`true` ⇒ 含**全部子孙**。
   */
  const includeChildren = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const panelOpen = ref(false);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let unlisten: (() => void)[] = [];
  let subscribed = false;
  let notesSeq = 0;

  /** 过滤后的树（先排序再过滤：过滤保留祖先链，行顺序因此与整树一致）。 */
  const visibleTree = computed(() => filterTree(sortTree(tree.value, sortMode.value), filter.value));
  const rows = computed<TagRow[]>(() => flattenTree(visibleTree.value));
  const totalTags = computed(() => walkTree(tree.value).filter((n) => !n.unclassified).length);
  const selectedNode = computed(() =>
    selectedNorm.value === null ? null : findTagNode(tree.value, selectedNorm.value),
  );
  const hasFilter = computed(() => filter.value.trim().length > 0);

  /** 拉一次整棵树（含末尾的「未分类」合成节点）。 */
  async function refresh(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      tree.value = await tagTree();
    } catch (err) {
      // R-15：不空吞 —— 面板必须显示失败原因，而不是假装"没有标签"。
      error.value = asKpError(err).message;
      tree.value = [];
    } finally {
      loading.value = false;
    }
  }

  /** 拉某个标签下的笔记；`norm` 传空串 = 「未分类」查询。 */
  async function loadNotes(norm: string): Promise<void> {
    notesSeq += 1;
    const seq = notesSeq;
    notesLoading.value = true;
    error.value = null;
    try {
      const items = await tagNotes(norm, includeChildren.value);
      if (seq !== notesSeq) return; // 过期响应：整包丢弃
      notes.value = items;
    } catch (err) {
      if (seq !== notesSeq) return;
      error.value = asKpError(err).message;
      notes.value = [];
    } finally {
      if (seq === notesSeq) notesLoading.value = false;
    }
  }

  /** 选中某标签（`null` 清空）。父组件据 `selectedNorm` watch 加载笔记。 */
  function select(norm: string | null): void {
    if (selectedNorm.value === norm) return;
    selectedNorm.value = norm;
    notes.value = [];
  }

  function setSortMode(mode: TagSortMode): void {
    sortMode.value = mode;
  }

  function setFilter(value: string): void {
    filter.value = value;
  }

  /** 切换"含子级"：**参数由调用方解析**（模板的 change 事件给的是 Event，TS 看不见）。
   *
   * 语义（PRD §5.3.6.2）：`false` = 只列显式写了该标签的笔记；`true` = 该标签 + 全部子孙。
   * 切换后立即重查当前选中的标签（与筛选/排序不同：这个参数在**后端**生效，必须重新发请求）。
   */
  function setIncludeChildren(flag: boolean): void {
    includeChildren.value = flag;
    if (selectedNorm.value !== null) void loadNotes(selectedNorm.value);
  }

  /** 重新拉树，并在有选中项时重拉它的笔记（改写落地后调用：AC-TAG-02 要求树随之更新）。 */
  async function reloadActive(): Promise<void> {
    await refresh();
    if (selectedNorm.value !== null) await loadNotes(selectedNorm.value);
  }

  /**
   * 面板入口 + 选中一个标签（FR-TAG-05：正文里点标签跳到"该标签的笔记列表视图"）。
   *
   * 入参是**正文里写的原文**（可能大写、可能带尾部斜杠）：这里只做"与索引同口径"的规范化，
   * **不去猜**层级或前缀（猜错就等于点进一个空列表）。
   */
  function selectTag(rawName: string): void {
    panelOpen.value = true;
    select(normalizeTagName(rawName));
  }

  function openPanel(): void {
    panelOpen.value = true;
  }

  function closePanel(): void {
    panelOpen.value = false;
  }

  /** 防抖刷新（索引/文件事件可能连发多条）。 */
  function scheduleRefresh(): void {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void refresh();
    }, REFRESH_DEBOUNCE_MS);
  }

  /** 订阅索引与文件事件（幂等：组件重复挂载不会叠加订阅）。 */
  async function subscribe(): Promise<void> {
    if (subscribed) return;
    subscribed = true;
    unlisten = await Promise.all([
      onIndexCompleted(() => scheduleRefresh()),
      onFsCreated(() => scheduleRefresh()),
      onFsModified(() => scheduleRefresh()),
      onFsRemoved(() => scheduleRefresh()),
      onFsRenamed(() => scheduleRefresh()),
    ]);
  }

  /** 卸载时退订并清掉待触发的防抖（不留悬挂订阅）。 */
  function dispose(): void {
    if (timer) clearTimeout(timer);
    timer = null;
    for (const off of unlisten) off();
    unlisten = [];
    subscribed = false;
  }

  // 与项目既有 store 一致：setup store 返回**普通对象**（Pinia 负责解包 ref）。
  return {
    tree, sortMode, filter, selectedNorm, notes, notesLoading, includeChildren,
    loading, error, panelOpen,
    visibleTree, rows, totalTags, selectedNode, hasFilter,
    refresh, reloadActive, loadNotes, select, selectTag, setSortMode, setFilter, setIncludeChildren,
    openPanel, closePanel, scheduleRefresh, subscribe, dispose,
  };
});
