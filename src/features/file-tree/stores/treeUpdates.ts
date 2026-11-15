/**
 * 文件树的**节点级更新**：本地增删改 + 外部 mtime 同步。
 *
 * 从 fileTree store 拆出（CODE-11 文件长度纪律）：这里只操作 cache / expanded / rows，
 * 通过 TreeUpdatePorts 注入状态，**不 import store**（避免循环依赖），store 只留编排。
 */
import type { Ref } from 'vue';
import type { FileNode } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import { baseName, insertChild, moveSubtree, parentOf, removeSubtree } from './treeCache';
import type { LocalNodeInit, SortDirection, TreeRow, TreeSortMode } from '../types';

/** treeUpdates 需要的最小状态接口。 */
export interface TreeUpdatePorts {
  cache: Map<string, FileNode[]>;
  expanded: Ref<Set<string>>;
  rows: Ref<TreeRow[]>;
  error: Ref<string | null>;
  sortMode: Ref<TreeSortMode>;
  sortDirection: Ref<SortDirection>;
  isStale: (gen: number) => boolean;
  generation: () => number;
  loadChildren: (relPath: string, gen: number) => Promise<void>;
  sort: (nodes: readonly FileNode[]) => FileNode[];
  flatten: () => TreeRow[];
}

/**
 * **定向重拉某文件所在的一层**（外部新建/变更后由 kp://fs/* 事件驱动）。
 * 只刷新**已加载**的父层：未加载的层展开时会自然取到新数据，不必提前拉。
 */
export async function refreshTreePath(ports: TreeUpdatePorts, relPath: string): Promise<void> {
  const parent = parentOf(relPath);
  if (!ports.cache.has(parent)) return;
  const gen = ports.generation();
  try {
    await ports.loadChildren(parent, gen);
    if (ports.isStale(gen)) return;
    ports.rows.value = ports.flatten();
  } catch (err) {
    if (!ports.isStale(gen)) ports.error.value = asKpError(err).message;
  }
}

/**
 * **外部 mtime 变更**（kp://fs/modified）：把树里对应节点的 mtime 换成事件里带的磁盘值并重排。
 *
 * 为什么不能只靠"重新拉树"：文件树是快照 + 懒加载缓存，而排序是对**缓存**排序；
 * 运行期外部改了文件的 mtime（或应用内保存了笔记）后，若不更新缓存，切到"修改时间"仍按旧值排
 * （M5 冒烟 5.3 的另一个可能来源）。事件载荷已带 mtimeMs，这里零 IPC。
 */
export function applyNodeMtime(ports: TreeUpdatePorts, relPath: string, mtimeMs: number): void {
  const parent = parentOf(relPath);
  const siblings = ports.cache.get(parent);
  if (!siblings) return;
  const index = siblings.findIndex((node) => node.relPath === relPath);
  const current = index < 0 ? undefined : siblings[index];
  if (!current || current.mtimeMs === mtimeMs) return;
  const next = [...siblings];
  next[index] = { ...current, mtimeMs };
  ports.cache.set(parent, ports.sort(next));
  ports.rows.value = ports.flatten();
}

/** 本地组装新节点（新建笔记/文件夹；size/kind 细分等索引刷新校正）。 */
function localNodeOf(relPath: string, init: LocalNodeInit): FileNode {
  const isDir = init.isDir ?? false;
  return {
    relPath,
    name: baseName(relPath),
    isDir,
    kind: isDir ? 'other' : 'note',
    ...(isDir ? { hasChildren: false } : {}),
    ...(init.mtimeMs === undefined ? {} : { mtimeMs: init.mtimeMs }),
  };
}

/** 新建成功后**本地插入**节点到已加载的父层并重排（父层由调用方先 ensureTreeVisible）。 */
export function insertLocalNode(ports: TreeUpdatePorts, relPath: string, init: LocalNodeInit): void {
  insertChild(ports.cache, localNodeOf(relPath, init), ports.sortMode.value, ports.sortDirection.value);
  ports.rows.value = ports.flatten();
}

/** 删除后**本地摘除**节点及其子树（不等索引刷新）。 */
export function removeTreeNode(ports: TreeUpdatePorts, relPath: string): void {
  ports.expanded.value = removeSubtree(ports.cache, ports.expanded.value, relPath);
  ports.rows.value = ports.flatten();
}

/** 重命名/移动后**本地改路径**（含已加载子树与展开状态；不等索引刷新）。 */
export function moveTreeNode(ports: TreeUpdatePorts, from: string, to: string): void {
  ports.expanded.value = moveSubtree(
    ports.cache,
    ports.expanded.value,
    from,
    to,
    ports.sortMode.value,
    ports.sortDirection.value,
  );
  ports.rows.value = ports.flatten();
}
