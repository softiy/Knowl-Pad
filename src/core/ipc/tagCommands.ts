//! 标签域的 IPC 绑定与形状（PRD **§5.3.6** 的命令表 + **§5.3.6.1 / §5.3.6.2** 的字段表）。
//!
//! 只含**本轮已实现**的两条只读命令（`tag_tree` / `tag_notes`）：`tag_suggest` 与 `tag_delete_apply`
//! 契约仍在但实现缺位（PRD 勘误 **D-38**），所以这里**不导出**它们 —— 免得前端 import 到一个永远不会成功的命令。
//! 写操作（`tag_rename_preview` / `tag_rename_apply`）属 PR-B，届时在 links 域的两步确认里接。

import { call } from './client';

/**
 * 标签树节点（FR-TAG-01/02；AC-TAG-01）。
 *
 * **计数口径**：`refCount` 是 **file_tag 行数**（不是笔记篇数）—— 因为 MD-TAG-03 要求层级标签同时登记
 * 自身与全部祖先，**祖先的计数天然已含全部子孙**，面板直接显示它即可（**不要再逐级相加**，那会重复计数）。
 * 「未分类」条目的 `refCount` 是**笔记篇数**（它没有 file_tag 行），单位不同，由 `unclassified` 区分。
 */
export interface TagNode {
  /** 小写规范键（`a/b/c`）。 */
  norm: string;
  /** 显示名（取自 `tag.display`）。 */
  display: string;
  /** 层级深度（`a/b/c` = 3）。 */
  depth: number;
  /** 是否末级（AC-TAG-01「仅最末级为真」）。 */
  isLeaf: boolean;
  /** 引用计数（行数口径；见上）。 */
  refCount: number;
  /** 仅「未分类」条目为 true（FR-TAG-13）；它的 `norm` 是空串且排在最后。 */
  unclassified: boolean;
  children: TagNode[];
}

/** 某标签下的一篇笔记（FR-TAG-03）。 */
export interface TaggedNote {
  relPath: string;
  name: string;
  mtimeMs: number;
}

/** 标签树（FR-TAG-01/02/13）；含末尾的「未分类」合成节点。 */
export const tagTree = (): Promise<TagNode[]> => call<TagNode[]>('tag_tree');

/**
 * 某标签下的笔记（FR-TAG-03）。
 *
 * `tagNorm` 传**空串** ⇒ 「未分类」查询（列出无任何标签的笔记），与树里 `unclassified` 节点一一对应。
 */
export const tagNotes = (tagNorm: string, includeChildren = false): Promise<TaggedNote[]> =>
  call<TaggedNote[]>('tag_notes', { args: { tagNorm, includeChildren } });
