import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import { onFsCreated, onFsModified, onFsRemoved, onFsRenamed, onIndexCompleted } from '@core/ipc/events';
import {
  linkAmbiguousList,
  linkBacklinks,
  linkDanglingList,
  linkOrphanList,
  type AmbiguousItem,
  type BacklinkGroup,
  type DanglingItem,
} from '@core/ipc/linkCommands';
import { asKpError } from '@core/ipc/errors';

/** 变更后合并刷新的窗口：远小于 FR-LINK-17 的「1 秒内」。 */
const REFRESH_DEBOUNCE_MS = 250;

/**
 * 链接面板的数据源（FR-LINK-10~21）。
 *
 * **数据必须来自索引库实时查询**（FR-LINK-17 明文），因此这里不做本地缓存推断：
 * 每次刷新都重新查询；索引/文件事件到来后**防抖 250ms** 再查，保证 1 秒内可见。
 */
export const useLinksStore = defineStore('links', () => {
  const backlinks = ref<BacklinkGroup[]>([]);
  const dangling = ref<DanglingItem[]>([]);
  const ambiguous = ref<AmbiguousItem[]>([]);
  const orphans = ref<string[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** 当前反链面板对应的笔记（为空则面板显示提示而不是空列表）。 */
  const activeRelPath = ref<string | null>(null);
  const lastRefreshedAt = ref(0);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let unlisten: (() => void)[] = [];

  const totals = computed(() => ({
    backlinks: backlinks.value.reduce((n, g) => n + g.linkCount, 0),
    embeds: backlinks.value.reduce((n, g) => n + g.embedCount, 0),
    dangling: dangling.value.length,
    ambiguous: ambiguous.value.length,
    orphans: orphans.value.length,
  }));

  /** 拉一次全部面板数据（索引库实时查询）。 */
  async function refresh(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      const [d, a, o] = await Promise.all([linkDanglingList(), linkAmbiguousList(), linkOrphanList()]);
      dangling.value = d.items;
      ambiguous.value = a.items;
      orphans.value = o.items;
      if (activeRelPath.value) {
        backlinks.value = await linkBacklinks(activeRelPath.value);
      }
      lastRefreshedAt.value = Date.now();
    } catch (err) {
      error.value = asKpError(err).message;
    } finally {
      loading.value = false;
    }
  }

  /** 切换反链面板的目标笔记。 */
  async function setActive(relPath: string | null): Promise<void> {
    activeRelPath.value = relPath;
    if (!relPath) {
      backlinks.value = [];
      return;
    }
    try {
      backlinks.value = await linkBacklinks(relPath);
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /** 防抖刷新（事件可能连发多条）。 */
  function scheduleRefresh(): void {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void refresh();
    }, REFRESH_DEBOUNCE_MS);
  }

  /** 订阅索引与文件事件。 */
  async function subscribe(): Promise<void> {
    unlisten = await Promise.all([
      onIndexCompleted(() => scheduleRefresh()),
      onFsCreated(() => scheduleRefresh()),
      onFsModified(() => scheduleRefresh()),
      onFsRemoved(() => scheduleRefresh()),
      onFsRenamed(() => scheduleRefresh()),
    ]);
  }

  function dispose(): void {
    if (timer) clearTimeout(timer);
    timer = null;
    for (const off of unlisten) off();
    unlisten = [];
  }

  // 与项目既有 store 一致：setup store 返回**普通对象**（Pinia 负责解包 ref）。
  return {
    backlinks,
    dangling,
    ambiguous,
    orphans,
    loading,
    error,
    activeRelPath,
    lastRefreshedAt,
    totals,
    refresh,
    setActive,
    scheduleRefresh,
    subscribe,
    dispose,
  };
});
