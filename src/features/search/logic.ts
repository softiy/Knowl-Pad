/**
 * 搜索域的**纯逻辑**（FR-SEARCH-01~11）：不碰 DOM、不碰 Pinia，因此可以直接单测。
 *
 * 单独成文件的两个理由：① CODE-11 的函数级上限是 80 SLOC，组件与 store 里塞不下这些分支；
 * ② 这里的判定（错误分类、防抖取消、选中项）正是最容易写错、也最该被测的部分。
 */

import { asKpError, isSearchSyntaxError } from '@core/ipc/errors';
import type { SearchResultItem, SearchSnippet } from '@core/ipc/searchCommands';

/** 输入防抖窗口（FR-SEARCH-08：防抖 200ms，且能取消上一次）。 */
export const SEARCH_DEBOUNCE_MS = 200;

/** 结果排序（FR-SEARCH-06：相关度默认；其余三种由后端排）。 */
export type SearchSort = 'relevance' | 'mtime' | 'name' | 'path';

export const SORT_OPTIONS: ReadonlyArray<{ value: SearchSort; label: string }> = [
  { value: 'relevance', label: '相关度' },
  { value: 'mtime', label: '修改时间' },
  { value: 'name', label: '文件名' },
  { value: 'path', label: '路径' },
];

/**
 * 错误呈现的**两种情形**（项目所有者裁定的过渡做法）。
 *
 * 背景：语法错误与"候选过多"共用 `E_SEARCH_SYNTAX` 这一个错误码，因此**只能按 message 区分**。
 * - `syntax`：用户自身语法写错（未闭合引号等）⇒ 呈现为"语法提示"；
 * - `too-broad`：查询合法但候选太多（后端文案含「当前候选 N 条、上限 300 条」）⇒ 原样呈现后端文案，
 *   因为文案里带着**具体数字**，是用户判断"该再收窄多少"的唯一依据；
 * - `other`：其余错误，照常显示 message。
 *
 * ⚠️ 这是**过渡实现**：真正的解法是后端给出独立的错误码（已作为待裁定项登记）。
 * 在那之前，这里的匹配串必须与后端文案同步改 —— 见 `src-tauri/src/commands/search.rs`。
 */
export type SearchErrorKind = 'syntax' | 'too-broad' | 'other';

/** 后端"候选过多"文案的特征串（search.rs 的 too_many_line_candidates）。 */
const TOO_BROAD_MARKERS = ['候选', '上限'];

export interface ClassifiedError {
  kind: SearchErrorKind;
  /** 面向用户的一行标题。 */
  title: string;
  /** 正文：语法错误给示例，候选过多原样透出后端文案（含具体数字）。 */
  message: string;
}

export function classifySearchError(err: unknown): ClassifiedError {
  const message = asKpError(err).message;
  if (!isSearchSyntaxError(err)) {
    return { kind: 'other', title: '搜索失败', message };
  }
  if (TOO_BROAD_MARKERS.some((m) => message.includes(m))) {
    return { kind: 'too-broad', title: '结果范围太大', message };
  }
  return {
    kind: 'syntax',
    title: '语法提示',
    message: `${message}（示例："完整短语" 用引号、tag:标签 限标签、path:目录/ 限路径）`,
  };
}

/**
 * "只认最后一次请求"的守卫（FR-SEARCH-08 的"取消上一次"）。
 *
 * 为什么不是真的 abort：IPC 调用一旦发出就无法取消，能做的只有**丢弃过期响应**，
 * 从而避免"慢的旧响应盖掉快的新响应"。返回的 `isCurrent` 就是丢弃判据。
 */
export function createLatestGuard(): {
  next: () => number;
  isCurrent: (id: number) => boolean;
} {
  let latest = 0;
  return {
    next: () => {
      latest += 1;
      return latest;
    },
    isCurrent: (id: number) => id === latest,
  };
}

/** 键盘导航：把选中项移动 `delta` 格，**夹紧**在 [0, length-1]（不环绕，避免"到底又跳回顶部"）。 */
export function nextIndex(current: number, length: number, delta: number): number {
  if (length <= 0) return -1;
  const raw = (current < 0 ? (delta > 0 ? 0 : length - 1) : current + delta);
  return Math.min(Math.max(raw, 0), length - 1);
}

/** 结果里可展开的片段（第 1 条常显；展开后显示其余）。 */
export function visibleSnippets(item: SearchResultItem, expanded: boolean): SearchSnippet[] {
  return expanded ? item.snippets : item.snippets.slice(0, 1);
}

/** 命中次数文案（FR-SEARCH-05：同一笔记多处命中合并为一条并显示次数）。 */
export function hitCountLabel(item: SearchResultItem): string {
  return item.hitCount > 1 ? `${item.hitCount} 处命中` : '1 处命中';
}

/** 修改时间（本地时区；未知时给占位符而不是伪造时间）。 */
export function formatMtime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '时间未知';
  const d = new Date(ms);
  const pad = (n: number): string => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 命中片段的高亮切分（`highlightStart/End` 是相对 `highlightLine` 的**字符**偏移、左闭右开）。 */
export function splitHighlight(
  text: string,
  start: number,
  end: number,
): { before: string; hit: string; after: string } {
  const s = Math.min(Math.max(start, 0), text.length);
  const e = Math.min(Math.max(end, s), text.length);
  return { before: text.slice(0, s), hit: text.slice(s, e), after: text.slice(e) };
}

/** 空结果时的建议（FR-SEARCH-11）。 */
export const EMPTY_HINTS: readonly string[] = [
  '换个更短的关键词，或用 "引号" 搜完整短语',
  '用 tag:标签 或 path:目录/ 先缩小范围',
  '用 -词 排除某个词，用 OR 连接候选（默认 AND）',
];
