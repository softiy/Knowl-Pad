/**
 * 标签域的**纯逻辑**（FR-TAG-01/02/04/13）：不碰 DOM、不碰 Pinia，因此可以直接单测。
 *
 * 单独成文件的两个理由（同搜索域 `logic.ts`）：① CODE-11 的函数级上限是 80 SLOC，
 * 组件与 store 里塞不下这些分支；② 这里的判定（计数口径、排序、过滤、选中）正是最容易写错、
 * 也最该被测的部分 —— 其中"不要再逐级相加"是 PRD §5.3.6.1 明确点名的重复计数陷阱。
 */

import type { TagNode } from '@core/ipc/tagCommands';

/** 标签树的排序方式（FR-TAG-04：按引用计数 / 按名称）。 */
export type TagSortMode = 'refCount' | 'name';

export const TAG_SORT_OPTIONS: ReadonlyArray<{ value: TagSortMode; label: string }> = [
  { value: 'refCount', label: '引用计数' },
  { value: 'name', label: '名称' },
];

/** 缺省排序（PRD §3.3 的标签面板查询就是 `ORDER BY ref_count DESC`）。 */
export const DEFAULT_TAG_SORT: TagSortMode = 'refCount';

/** 扁平化后的一行（树形渲染用：缩进靠 `depth`，"有没有下级"靠 `hasChildren`）。 */
export interface TagRow {
  /** 稳定行键（`norm` 在整棵树里唯一；「未分类」用它的 display 兜底）。 */
  key: string;
  node: TagNode;
  /** 行在树里的层级（**1 基**，与 `TagNode.depth` 同一口径：`a/b/c` 的 a 是 1）。 */
  depth: number;
  hasChildren: boolean;
}

/**
 * 比较两个节点（FR-TAG-04）。
 *
 * 两条口径都不是随手定的：
 * ① `unclassified` **恒排最后** —— PRD §5.3.6.1 明定「排在全部标签之后」，
 *    它必须**压过**排序方式（按引用计数时它往往是最大值，按名字时"未分类"也不该插在中间）；
 * ② `refCount` 是 **file_tag 行数**，祖先的计数**天然已含全部子孙**（MD-TAG-03 的祖先登记）
 *    ⇒ 这里**只读不累加**：再逐级相加就是 PRD 点名的重复计数。
 */
export function compareTagNodes(a: TagNode, b: TagNode, mode: TagSortMode): number {
  if (a.unclassified !== b.unclassified) return a.unclassified ? 1 : -1;
  if (mode === 'name') return a.display.localeCompare(b.display);
  if (a.refCount !== b.refCount) return b.refCount - a.refCount;
  return a.display.localeCompare(b.display);
}

/** 排序后的副本（不改动入参：store 里的树是共享状态）。 */
export function sortTree(nodes: TagNode[], mode: TagSortMode = DEFAULT_TAG_SORT): TagNode[] {
  return [...nodes]
    .sort((a, b) => compareTagNodes(a, b, mode))
    .map((node) => ({ ...node, children: sortTree(node.children, mode) }));
}

/** 行键（`norm` 唯一；「未分类」的 norm 是空串，故退化用 display）。 */
export function rowKeyOf(node: TagNode): string {
  return node.norm !== '' ? node.norm : `unclassified:${node.display}`;
}

/** 扁平化成渲染行（前序遍历：父先于子，与缩进视觉一致）。 */
export function flattenTree(nodes: TagNode[], level = 1): TagRow[] {
  const rows: TagRow[] = [];
  for (const node of nodes) {
    rows.push({ key: rowKeyOf(node), node, depth: level, hasChildren: node.children.length > 0 });
    rows.push(...flattenTree(node.children, level + 1));
  }
  return rows;
}

/** 全树节点的前序遍历（供"按 norm 找节点"与统计复用）。 */
export function walkTree(nodes: TagNode[]): TagNode[] {
  return nodes.flatMap((node) => [node, ...walkTree(node.children)]);
}

/** 按 `norm` 找节点（「未分类」传空串）；找不到返回 null。 */
export function findTagNode(nodes: TagNode[], norm: string): TagNode | null {
  return walkTree(nodes).find((node) => node.norm === norm) ?? null;
}

/** 节点是否命中过滤串（大小写不敏感；同时匹配**完整规范键**与**显示名**）。 */
export function matchesFilter(node: TagNode, needle: string): boolean {
  const q = needle.trim().toLowerCase();
  if (!q) return true;
  return node.norm.toLowerCase().includes(q) || node.display.toLowerCase().includes(q);
}

/** 该节点自身或其任一子孙命中（祖先必须留下，否则树会被截断成孤儿子节点）。 */
export function subtreeMatches(node: TagNode, needle: string): boolean {
  return matchesFilter(node, needle) || node.children.some((child) => subtreeMatches(child, needle));
}

/**
 * 按过滤串裁剪树（FR-TAG-04 的"面板内过滤"）。
 *
 * 保留**命中节点的全部祖先链**：只留叶子会让 `a/b/c` 显示成没有 `a`、`b` 的一行，
 * 用户就再也看不出层级了（FR-TAG-02 的树形语义优先于"最小结果集"）。
 */
export function filterTree(nodes: TagNode[], needle: string): TagNode[] {
  if (!needle.trim()) return nodes;
  const kept: TagNode[] = [];
  for (const node of nodes) {
    if (!subtreeMatches(node, needle)) continue;
    // 自身不命中时子孙可能命中 ⇒ 递归裁剪；自身命中时同样递归，保持"过滤后仍是同一棵树"。
    kept.push({ ...node, children: filterTree(node.children, needle) });
  }
  return kept;
}

/** 选中判定：按 `norm`（「未分类」用空串，与 `tag_notes` 的入参一致）。 */
export function isSelectedNode(node: TagNode, selectedNorm: string | null): boolean {
  return selectedNorm !== null && node.norm === selectedNorm;
}

/**
 * 「未分类」条目的计数单位是**篇数**（它没有 file_tag 行），其余是行数 ——
 * 单位不同，展示文案也不该一样（PRD §5.3.6.1 的"计数口径"④）。
 */
export function refCountLabel(node: TagNode): string {
  return node.unclassified ? `${node.refCount} 篇` : `${node.refCount} 次`;
}

/**
 * 笔记修改时间（本地时区；未知时给占位符而**不伪造**时间）。
 *
 * 与搜索域的 `formatMtime` 是同一条口径，但**不跨 feature import**（FE-02 禁止 feature 互相依赖）；
 * 与其把它挪进 core 造一个只有两个调用点的新共享点，不如各自留一份 4 行实现。
 */
export function formatNoteTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '时间未知';
  const d = new Date(ms);
  const pad = (n: number): string => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
