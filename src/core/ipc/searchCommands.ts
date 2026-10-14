//! 搜索域的 IPC 绑定与形状（PRD §5.3.4 的 6 个命令）。
//!
//! 单独成文件：commands.ts 已 260 行上下，再把搜索域塞进去会逼近 CODE-11 的 350 硬上限
//! （与 linkCommands.ts 同样的理由）。
//!
//! 形状来源：PRD §5.3.4 只给出返回类型的**名字**，字段口径落在 Rust 侧
//! `src-tauri/src/commands/search_view.rs`（每字段注明需求出处）；两处必须同步改。

import { call, callVoid } from './client';

/** 片段中的一行。 */
export interface SnippetLine {
  line: number;
  text: string;
  /** 命中所在行 */
  isHitLine: boolean;
}

/** 命中片段：highlightStart/highlightEnd 相对 highlightLine 的文本，0 基、左闭右开、**字符**偏移。 */
export interface SearchSnippet {
  lines: SnippetLine[];
  highlightLine: number;
  highlightStart: number;
  highlightEnd: number;
}

/** 一条搜索结果（同一笔记多处命中合并为一条；片段首个即 snippets[0]，FR-SEARCH-04/05）。 */
export interface SearchResultItem {
  relPath: string;
  name: string;
  mtimeMs: number;
  /** bm25 相关度（负值，越小越相关） */
  score: number;
  hitCount: number;
  snippets: SearchSnippet[];
}

/** 一页结果（PRD §5.3.4 的 SearchResultPage）。 */
export interface SearchResultPage {
  items: SearchResultItem[];
  /** 命中总数（不是本页条数） */
  total: number;
  offset: number;
  limit: number;
  hasMore: boolean;
}

/** 全文搜索选项（字段出处见 Rust 侧 SearchOptions 的逐条注释）。 */
export interface SearchOptions {
  /** 缺省 all（PRD §4.11.3「默认匹配模式 AND」） */
  matchMode?: 'all' | 'any';
  /** 缺省 relevance（FR-SEARCH-06） */
  sort?: 'relevance' | 'mtime' | 'name' | 'path';
  /** 片段上下文行数，缺省 2（PRD §4.11.3） */
  contextLines?: number;
  /** 每条结果最多几个片段（IPC-05 的载荷控制） */
  maxSnippets?: number;
  /** 是否把本次查询记入历史（FR-SEARCH-10）；**缺省 false**，只在用户确认查询时置 true */
  recordHistory?: boolean;
}

/** 分页：缺省 offset 0 / limit 20。 */
export interface SearchPage {
  offset?: number;
  limit?: number;
}

/** 快速打开候选（PRD §5.3.4 的 QuickOpenItem）。 */
export interface QuickOpenItem {
  relPath: string;
  name: string;
  /** 模糊匹配得分（越大越靠前）；口径见 kp_domain::quick_open */
  score: number;
}

/** 文件内一处命中（PRD §5.3.4 的 InFileMatch）。 */
export interface InFileMatch {
  line: number;
  /** 1 基**字符**列 */
  col: number;
  lenChars: number;
  /** 原文中的字节区间：replace_in_file 按它落刀 */
  startByte: number;
  endByte: number;
  text: string;
  lineText: string;
  /** 相对 lineText 的高亮区间（0 基、左闭右开、字符） */
  highlightStart: number;
  highlightEnd: number;
}

/** 文件内查找选项（FR-EDITOR-09；FR-SEARCH-31 复用）。 */
export interface FindOptions {
  caseSensitive?: boolean;
  wholeWord?: boolean;
}

/** 替换结果（PRD §5.3.4 的 ReplaceResult）。 */
export interface ReplaceResult {
  relPath: string;
  replaced: number;
  newMtimeMs: number;
}

/** 全文搜索（FR-SEARCH-01~09）。 */
export const searchFulltext = (
  query: string,
  options?: SearchOptions,
  page?: SearchPage,
): Promise<SearchResultPage> => call<SearchResultPage>('search_fulltext', { args: { query, options, page } });

/** 快速打开（FR-SEARCH-40/41：只匹配文件名/路径，不搜正文）。 */
export const searchQuickOpen = (query: string, limit?: number): Promise<QuickOpenItem[]> =>
  call<QuickOpenItem[]>('search_quick_open', { args: { query, limit } });

/** 文件内查找（FR-SEARCH-30：「第 X / 共 Y 处」= 下标与数组长度）。 */
export const searchInFile = (
  relPath: string,
  query: string,
  options?: FindOptions,
): Promise<InFileMatch[]> => call<InFileMatch[]>('search_in_file', { args: { relPath, query, options } });

/** 文件内替换（⚠️ 写操作，FR-SEARCH-31）：matches 直接传 searchInFile 的返回值（含期望原文）。 */
export const replaceInFile = (
  relPath: string,
  matches: InFileMatch[],
  replacement: string,
): Promise<ReplaceResult> =>
  call<ReplaceResult>('replace_in_file', { args: { relPath, matches, replacement } });

/** 搜索历史（FR-SEARCH-10）：最近 20 条，最新在前。 */
export const searchHistoryGet = (): Promise<string[]> => call<string[]>('search_history_get');

/** 清空搜索历史（⚠️ 写操作）：UI 必须先二次确认（PRD §5.3 的 ⚠️ 约定）。 */
export const searchHistoryClear = (): Promise<void> => callVoid('search_history_clear');
