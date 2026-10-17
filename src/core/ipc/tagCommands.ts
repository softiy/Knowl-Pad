//! 标签域的 IPC 绑定与形状（PRD **§5.3.6** 的命令表 + **§5.3.6.1 / §5.3.6.2** 的字段表）。
//!
//! 只含**本轮已实现**的两条只读命令（`tag_tree` / `tag_notes`）：`tag_suggest` 与 `tag_delete_apply`
//! 契约仍在但实现缺位（PRD 勘误 **D-38**），所以这里**不导出**它们 —— 免得前端 import 到一个永远不会成功的命令。
//! 写操作（`tag_rename_preview` / `tag_rename_apply`）属 PR-B，届时在 links 域的两步确认里接。

import { call } from './client';
import type { RewritePreview, RewriteResult } from './linkCommands';

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

/** 标签来源（FR-TAG-10 的「来源可区分」）：`body` = 正文里的 `#标签`，`frontmatter` = frontmatter 的 `tags:`。 */
export type TagSource = 'body' | 'frontmatter';

/** 某标签下的一篇笔记（FR-TAG-03）。 */
export interface TaggedNote {
  relPath: string;
  name: string;
  mtimeMs: number;
  /**
   * 这一篇在**哪些来源**写了该标签（FR-TAG-10；2026-10-17 批次 4 落地）。
   *
   * 固定顺序 `["body"]` / `["frontmatter"]` / `["body", "frontmatter"]`；
   * 空数组只出现在「未分类」查询（那批笔记本来就没有标签行）。
   */
  sources: TagSource[];
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
/**
 * 标签重命名**预览**（FR-TAG-07/08；只读，不写盘）。
 *
 * 返回**复用链接域的 `RewritePreview`**（裁定：不叉类型）—— 其中 `edits[].occurrences[]` 的语义是
 * 「该文件中标签出现的位置（正文行或 frontmatter 行）」。`toTag` 非法（空/首尾斜杠/连续斜杠/空白/循环）时
 * 后端返回 `E_INVALID_FILENAME`，磁盘零改动。
 */
export const tagRenamePreview = (fromTag: string, toTag: string): Promise<RewritePreview> =>
  call<RewritePreview>('tag_rename_preview', { args: { fromTag, toTag } });

/**
 * 标签批量重命名（**⚠️ 写操作**：UI 必须先走预览并二次确认）。
 *
 * `previewId` **一次性消费**（过期/已用 ⇒ `E_PREVIEW_EXPIRED`）；预览之后文件被外部改动 ⇒
 * `E_REWRITE_FAILED` 且磁盘零改动。整批"全有或全无"（AC-TAG-02）。
 */
export const tagRenameApply = (
  fromTag: string,
  toTag: string,
  previewId: string,
): Promise<RewriteResult> =>
  call<RewriteResult>('tag_rename_apply', { args: { fromTag, toTag, previewId } });
