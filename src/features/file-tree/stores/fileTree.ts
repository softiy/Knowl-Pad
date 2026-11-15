import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import { fileTree, vaultStateSet, type FileNode } from '@core/ipc/commands';
import { defaultDirectionOf, sortNodes } from '../logic';
import {
  STATE_KEY_TREE_EXPANDED,
  type LocalNodeInit,
  type SortDirection,
  type TreeRow,
  type TreeSortMode,
} from '../types';
import { flattenRows } from './treeCache';
import {
  ensureTreeVisible,
  refreshKeepingExpanded,
  restoreTreeState,
  setIncludeHiddenAndReload,
} from './treeIO';
import {
  applyNodeMtime,
  insertLocalNode,
  moveTreeNode,
  refreshTreePath,
  removeTreeNode,
} from './treeUpdates';

const ROOT = '';

/**
 * 文件树 store：**扁平化 + 懒加载**（技术方案 §7.3）。
 *
 * 设计要点：
 * - 只缓存**已加载层级**（`cache` 非响应式），展开时才请求子节点 → 10 万节点不会一次性进内存；
 * - `rows` 是扁平化可视行数组，虚拟滚动直接消费；
 * - 展开状态持久化到 `vault_state`（AC-FILE-08），隐藏文件开关持久化到本 Vault 的 `vault_state`（FR-FILE-05）；
 * - **装载代次**（`generation`）：切库/reset 后，上一个库还在飞的请求返回时必须作废，
 *   否则会把旧库的目录写进新库的缓存（冒烟里的「串文件树 / 文件不存在：归档 不是目录」）；
 * - 持久化 / 整树装载在 `treeIO.ts`、节点级增删改在 `treeUpdates.ts`、缓存原语在 `treeCache.ts`
 *   （CODE-11 的文件长度纪律）；store 只留状态与编排。
 */
export const useFileTreeStore = defineStore('fileTree', () => {
  const rows = ref<TreeRow[]>([]);
  const expanded = ref<Set<string>>(new Set());
  const includeHidden = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** 已加载层级缓存：父目录相对路径 → 子节点（根为 ''）；**存入前已按当前排序模式排好** */
  const cache = new Map<string, FileNode[]>();
  /** FR-FILE-02：排序模式（默认名称；纯视图状态，不持久化 —— 见 DEBT-19 的处置说明） */
  const sortMode = ref<TreeSortMode>('name');
  /** 排序方向（M5 冒烟 5.3）：默认方向随模式而定，用户可再翻转（见 logic.ts::defaultDirectionOf） */
  const sortDirection = ref<SortDirection>(defaultDirectionOf('name'));
  let generation = 0;

  const isStale = (gen: number): boolean => gen !== generation;
  const flatten = (): TreeRow[] => flattenRows(cache, expanded.value);
  const sorted = (nodes: readonly FileNode[]): FileNode[] =>
    sortNodes(nodes, sortMode.value, sortDirection.value);

  /**
   * 三个拆分模块共用的状态视图：它们**不 import store**（避免循环依赖），
   * 只通过这个对象读写状态与装载原语（各模块自身声明所必需的子集，结构兼容）。
   */
  const ports = {
    cache,
    expanded,
    includeHidden,
    rows,
    loading,
    error,
    sortMode,
    sortDirection,
    isStale,
    sort: sorted,
    flatten,
    generation: () => generation,
    persistExpanded: () => persistExpanded(),
    loadChildren: (relPath: string, gen: number) => loadChildren(relPath, gen),
    expandPath: (relPath: string, gen?: number) => expandPath(relPath, gen),
  };

  /** 拉取一层子节点并写入缓存；代次变了（切库/reset）就丢弃结果。 */
  async function loadChildren(relPath: string, gen: number): Promise<void> {
    const nodes = await fileTree(relPath === ROOT ? undefined : relPath, includeHidden.value);
    if (isStale(gen)) return;
    cache.set(relPath, sorted(nodes));
  }

  /** 按当前模式与方向重排所有已加载层（纯前端重排，不重新请求）。 */
  function resortAll(): void {
    for (const [parent, nodes] of [...cache.entries()]) cache.set(parent, sorted(nodes));
    rows.value = flatten();
  }

  /** FR-FILE-02：切换排序 —— **纯前端重排，不重新请求**；方向回到该模式的默认值。 */
  function setSortMode(mode: TreeSortMode): void {
    if (sortMode.value === mode) return;
    sortMode.value = mode;
    sortDirection.value = defaultDirectionOf(mode);
    resortAll();
  }

  /** M5 冒烟 5.3：翻转当前模式的方向（文件夹永远在前的分组不翻转）。 */
  function toggleSortDirection(): void {
    sortDirection.value = sortDirection.value === 'asc' ? 'desc' : 'asc';
    resortAll();
  }

  /** 载入根层并重建可视行。 */
  async function loadRoot(): Promise<void> {
    const gen = ++generation;
    loading.value = true;
    error.value = null;
    try {
      cache.clear();
      await loadChildren(ROOT, gen);
      if (isStale(gen)) return;
      rows.value = flatten();
    } catch (err) {
      if (!isStale(gen)) error.value = asKpError(err).message;
    } finally {
      if (!isStale(gen)) loading.value = false;
    }
  }

  /** 逐层展开某路径（其祖先必须已加载）。 */
  async function expandPath(relPath: string, gen = generation): Promise<void> {
    let parent = ROOT;
    for (const segment of relPath.split('/')) {
      if (!segment) continue;
      const current = parent === ROOT ? segment : `${parent}/${segment}`;
      if (!cache.has(current)) {
        await loadChildren(current, gen);
        if (isStale(gen)) return;
      }
      expanded.value = new Set([...expanded.value, current]);
      parent = current;
    }
  }

  /** 持久化展开状态；失败不静默（R-15）。 */
  async function persistExpanded(): Promise<void> {
    try {
      await vaultStateSet({ [STATE_KEY_TREE_EXPANDED]: [...expanded.value].sort() });
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /** 展开/折叠目录（懒加载其子层）。 */
  async function toggle(relPath: string): Promise<void> {
    const next = new Set(expanded.value);
    if (next.has(relPath)) {
      next.delete(relPath);
      expanded.value = next;
      rows.value = flatten();
      await persistExpanded();
      return;
    }
    next.add(relPath);
    expanded.value = next;
    const gen = generation;
    try {
      if (!cache.has(relPath)) {
        await loadChildren(relPath, gen);
        if (isStale(gen)) return;
      }
      rows.value = flatten();
    } catch (err) {
      error.value = asKpError(err).message;
      expanded.value = new Set([...expanded.value].filter((path) => path !== relPath));
    }
    await persistExpanded();
  }

  /** 重新加载（手动刷新 / 索引追上后用）：清缓存 → 重载根 → 恢复原有展开层级。 */
  async function refresh(): Promise<void> {
    await refreshKeepingExpanded(ports, ++generation);
  }

  /** **定向重拉**某文件所在的一层（外部新建/变更后由 kp://fs/* 事件驱动）。 */
  async function refreshPath(relPath: string): Promise<void> {
    await refreshTreePath(ports, relPath);
  }

  /** 切换「显示隐藏文件」（FR-FILE-05）：口径与落盘位置见 treeIO.ts。 */
  async function setIncludeHidden(value: boolean): Promise<void> {
    await setIncludeHiddenAndReload(ports, ++generation, value);
  }

  /** 恢复上次的展开状态（AC-FILE-08）与隐藏文件偏好（FR-FILE-05）。 */
  async function restoreState(): Promise<void> {
    await restoreTreeState(ports, ++generation);
  }

  /**
   * **切库重置**（layout 在 Vault 切换时调用，M5 冒烟「切库后串文件树」）：
   * 清空展开状态、缓存、可视行与错误，隐藏文件开关回默认值。**不调用任何 IPC、不写 vault_state** ——
   * 新库的状态由随后的 `restoreState()` 从它自己的 vault_state 读，先写会把两个库的状态混在一起。
   */
  function reset(): void {
    generation += 1; // 让上一库还在飞的请求全部作废
    cache.clear();
    expanded.value = new Set();
    rows.value = [];
    error.value = null;
    loading.value = false;
    includeHidden.value = false;
    sortDirection.value = defaultDirectionOf(sortMode.value);
  }

  /**
   * 新建笔记/文件夹成功后**本地插入**节点（M5 冒烟 5.6「新建笔记未即时在文件树显示」）：
   * 索引由文件监听异步更新，操作后立刻重拉只会看到旧索引；因此先按命令结果改树，
   * 立即反馈给用户，等 refresh / 重开 Vault 时再由索引校正。
   */
  async function nodeAdded(relPath: string, init: LocalNodeInit = {}): Promise<void> {
    await ensureTreeVisible(ports, relPath);
    insertLocalNode(ports, relPath, init);
  }

  /** 删除成功后**本地移除**节点及其子树（同上：不等索引）。 */
  function pathRemoved(relPath: string): void {
    removeTreeNode(ports, relPath);
  }

  /** 重命名/移动成功后**本地改名**节点及其子树（同上：不等索引）。 */
  function pathMoved(from: string, to: string): void {
    moveTreeNode(ports, from, to);
  }

  /** **外部 mtime 变更**（kp://fs/modified）：更新缓存值并重排，零 IPC。 */
  function applyMtime(relPath: string, mtimeMs: number): void {
    applyNodeMtime(ports, relPath, mtimeMs);
  }

  return {
    rows,
    expanded,
    includeHidden,
    sortMode,
    sortDirection,
    setSortMode,
    toggleSortDirection,
    loading,
    error,
    loadRoot,
    toggle,
    setIncludeHidden,
    restoreState,
    refresh,
    persistExpanded,
    reset,
    refreshPath,
    applyMtime,
    nodeAdded,
    pathRemoved,
    pathMoved,
  };
});
