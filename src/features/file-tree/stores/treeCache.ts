/**
 * 文件树**本地缓存的可变操作**（从 fileTree store 拆出；CODE-11 的文件长度纪律）。
 *
 * 为什么需要"本地改树"：文件树默认数据源是**索引库**，而写操作之后索引由文件监听异步更新
 * （防抖 200ms + 事务）。若操作成功后立刻重新拉取，读到的仍是**旧索引** ——
 * 新建的笔记"不在树里"、删掉的笔记"还在树里"。因此 store 先按命令结果本地改树（即时反馈），
 * 索引稍后由 refresh / 重开 Vault 校正。本文件只做 Map/Set 操作，便于直接单测。
 *
 * 这里**不**负责响应式：cache / expanded 由 store 持有，本文件只提供纯数据变换。
 */
import type { FileNode } from '@core/ipc/commands';
import { sortNodes } from '../logic';
import type { SortDirection, TreeRow, TreeSortMode } from '../types';

const ROOT = '';

/** 父目录相对路径（根层为 ''）。 */
export function parentOf(relPath: string): string {
  const index = relPath.lastIndexOf('/');
  return index < 0 ? '' : relPath.slice(0, index);
}

/** 路径最后一段（文件/目录名）。 */
export function baseName(relPath: string): string {
  const index = relPath.lastIndexOf('/');
  return index < 0 ? relPath : relPath.slice(index + 1);
}

/** 依据 cache 与展开状态把树压平成一维可视行（虚拟滚动的数据源）。 */
export function flattenRows(
  cache: ReadonlyMap<string, readonly FileNode[]>,
  expanded: ReadonlySet<string>,
): TreeRow[] {
  const out: TreeRow[] = [];
  const walk = (parent: string, depth: number): void => {
    for (const node of cache.get(parent) ?? []) {
      const isExpanded = node.isDir && expanded.has(node.relPath);
      out.push({
        relPath: node.relPath,
        name: node.name,
        isDir: node.isDir,
        kind: node.kind,
        depth,
        hasChildren: node.hasChildren ?? false,
        expanded: isExpanded,
      });
      if (isExpanded) walk(node.relPath, depth + 1);
    }
  };
  walk(ROOT, 0);
  return out;
}

/**
 * 把节点插入其父目录的缓存层（父层**必须已加载**，否则返回 false —— 看不见的层不需要更新）。
 * 重复路径不会插两次（索引稍后追上时仍以本地这份为准，直到下一次完整 refresh）。
 */
export function insertChild(
  cache: Map<string, FileNode[]>,
  node: FileNode,
  mode: TreeSortMode,
  direction: SortDirection,
): boolean {
  const parent = parentOf(node.relPath);
  const siblings = cache.get(parent);
  if (!siblings) return false;
  if (siblings.some((existing) => existing.relPath === node.relPath)) return true;
  cache.set(parent, sortNodes([...siblings, node], mode, direction));
  return true;
}

/** 从父层缓存里摘掉一个节点（返回摘到的节点；父层未加载或节点不存在时返回 undefined）。 */
function takeNode(cache: Map<string, FileNode[]>, relPath: string): FileNode | undefined {
  const parent = parentOf(relPath);
  const siblings = cache.get(parent);
  if (!siblings) return undefined;
  const node = siblings.find((existing) => existing.relPath === relPath);
  if (!node) return undefined;
  cache.set(parent, siblings.filter((existing) => existing.relPath !== relPath));
  return node;
}

/**
 * 删除某路径**及其子树**的本地缓存与展开状态；返回过滤后的展开集合。
 * 子树的缓存层必须一起删 —— 否则同名路径再次出现时会读到上一个文件的旧子层。
 */
export function removeSubtree(
  cache: Map<string, FileNode[]>,
  expanded: ReadonlySet<string>,
  relPath: string,
): Set<string> {
  takeNode(cache, relPath);
  const prefix = relPath + '/';
  const isUnder = (path: string): boolean => path === relPath || path.startsWith(prefix);
  for (const key of [...cache.keys()]) {
    if (isUnder(key)) cache.delete(key);
  }
  return new Set([...expanded].filter((path) => !isUnder(path)));
}

/**
 * 移动/重命名后的本地树同步：把节点从旧父层挪到新父层，并让**已加载的子层缓存与展开状态**
 * 一起换路径（移动目录时不这么做，再次展开会按旧路径请求而报「不是目录」）。
 * 返回改写后的展开集合。
 */
export function moveSubtree(
  cache: Map<string, FileNode[]>,
  expanded: ReadonlySet<string>,
  from: string,
  to: string,
  mode: TreeSortMode,
  direction: SortDirection,
): Set<string> {
  const node = takeNode(cache, from);
  const prefix = from + '/';
  const isUnder = (path: string): boolean => path === from || path.startsWith(prefix);
  for (const key of [...cache.keys()]) {
    if (!isUnder(key)) continue;
    const movedKey = to + key.slice(from.length);
    // 缓存键与**节点自身的 relPath** 都要换：只改键不改节点，展开后行里还是旧路径（点开就报文件不存在）。
    const children = (cache.get(key) ?? []).map((child) => ({
      ...child,
      relPath: to + child.relPath.slice(from.length),
    }));
    cache.delete(key);
    cache.set(movedKey, children);
  }
  const nextExpanded = new Set([...expanded].map((path) => (isUnder(path) ? to + path.slice(from.length) : path)));
  if (node) {
    insertChild(cache, { ...node, relPath: to, name: baseName(to) }, mode, direction);
  }
  return nextExpanded;
}
