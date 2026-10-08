//! 链接域的 IPC 绑定与形状（PRD §5.3.5 查询 + §5.3.3 改写）。
//!
//! 单独成文件：`commands.ts` 已 260 行上下，把链接域（10 个命令）塞进去会逼近 CODE-11 的 350 硬上限。

import { call, callVoid } from './client';

/** 反链的上下文片段（FR-LINK-11/12）。 */
export interface SnippetLine {
  line: number;
  text: string;
  isLinkLine: boolean;
}
export interface Snippet {
  lines: SnippetLine[];
  highlightLine: number;
  highlightStart: number;
  highlightEnd: number;
}
export interface BacklinkItem {
  line: number;
  col: number;
  linkKind: string;
  alias?: string | null;
  anchor?: string | null;
  snippet?: Snippet | null;
}
/** 按来源分组的一组反链（FR-LINK-14/15：分组 + 普通/嵌入分开计数）。 */
export interface BacklinkGroup {
  srcRelPath: string;
  srcName: string;
  linkCount: number;
  embedCount: number;
  items: BacklinkItem[];
}
export interface OutgoingLink {
  targetRef: string;
  status: string;
  anchor?: string | null;
  alias?: string | null;
  linkKind: string;
  line: number;
  col: number;
  dstRelPath?: string | null;
}
export interface HeadingItem {
  level: number;
  text: string;
  anchor: string;
  line: number;
}
export interface DanglingItem {
  targetRef: string;
  refCount: number;
  sourceCount: number;
  sampleSources: string[];
}
export interface DanglingPage {
  items: DanglingItem[];
  total: number;
}
export interface AmbiguousItem {
  targetRef: string;
  candidates: string[];
  refCount: number;
}
export interface AmbiguousPage {
  items: AmbiguousItem[];
  total: number;
}
export interface OrphanPage {
  items: string[];
  total: number;
}

/** 反向链接（FR-LINK-10~15）。 */
export const linkBacklinks = (relPath: string, includeEmbeds = true): Promise<BacklinkGroup[]> =>
  call<BacklinkGroup[]>('link_backlinks', { relPath, includeEmbeds });
/** 出链。 */
export const linkOutgoing = (relPath: string): Promise<OutgoingLink[]> =>
  call<OutgoingLink[]>('link_outgoing', { relPath });
/** 悬空链接清单（FR-LINK-20）。 */
export const linkDanglingList = (): Promise<DanglingPage> => call<DanglingPage>('link_dangling_list');
/** 歧义链接清单（FR-LINK-22 / AC-LINK-04）。 */
export const linkAmbiguousList = (): Promise<AmbiguousPage> => call<AmbiguousPage>('link_ambiguous_list');
/** 孤立笔记清单（FR-LINK-21）。 */
export const linkOrphanList = (): Promise<OrphanPage> => call<OrphanPage>('link_orphan_list');
/** 标题列表（供 FR-EDITOR-23 标题补全）。 */
export const linkHeadings = (relPath: string): Promise<HeadingItem[]> =>
  call<HeadingItem[]>('link_headings', { relPath });

/** 歧义消解：把某条链接改写为完整相对路径（FR-LINK-22）。 */
export interface RewriteResult {
  operationId: string;
  fileCount: number;
  spanCount: number;
}
export const linkResolveAmbiguous = (linkId: number, targetRelPath: string): Promise<RewriteResult> =>
  call<RewriteResult>('link_resolve_ambiguous', { linkId, targetRelPath });

/** 改名规格（PRD §5.3.3 的 `rename: { from, to }`）。 */
export interface RenameSpec {
  from: string;
  to: string;
}
export interface RewritePreview {
  previewId: string;
  fromRef: string;
  toRef: string;
  rename?: RenameSpec | null;
  fileCount: number;
  spanCount: number;
  edits: { relPath: string; hits: number }[];
  createdAtMs: number;
}
/** 第一步：**只读**预览（FR-FILE-21 ②：将修改 N 个文件中的 M 处）。 */
export const linkRewritePreview = (
  fromRef: string,
  toRef: string,
  rename?: RenameSpec,
): Promise<RewritePreview> => call<RewritePreview>('link_rewrite_preview', { fromRef, toRef, rename });
/** 第二步：用户确认后执行（一次性消费 previewId；全有或全无）。 */
export const linkRewriteApply = (previewId: string, rename?: RenameSpec): Promise<RewriteResult> =>
  call<RewriteResult>('link_rewrite_apply', { previewId, rename });
/** 从备份回滚某次改写（AC-FILE-02）。 */
export const linkRewriteRollback = (operationId: string): Promise<void> =>
  callVoid('link_rewrite_rollback', { operationId });
