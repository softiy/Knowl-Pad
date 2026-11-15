import type { FindOptions, InFileMatch } from './types';

/** 一条替换改动（CodeMirror 事务的 changes 形状；从这里派生，避免适配器直接依赖内核类型）。 */
export interface TextChange {
  from: number;
  to: number;
  insert: string;
}

/**
 * 文件内查找（纯函数）。
 *
 * 为什么拆出来：查找/替换是纯文本运算，不依赖 DOM 与内核；拆开后可单测边界（正则、全字、大小写），
 * 适配器只负责"从内核取当前文本"。
 */
export function findInText(text: string, query: string, opts: FindOptions = {}): InFileMatch[] {
  if (!query) return [];
  const flags = opts.caseSensitive ? 'g' : 'gi';
  const pattern = opts.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const source = opts.wholeWord ? `\\b(?:${pattern})\\b` : pattern;
  const matches: InFileMatch[] = [];
  const re = new RegExp(source, flags);
  for (const m of text.matchAll(re)) {
    const from = m.index ?? 0;
    matches.push({ from, to: from + m[0].length, text: m[0] });
  }
  return matches;
}

/**
 * 批量替换（从后往前算，避免偏移失效）。
 *
 * 同时返回 codeMirror 事务里的 `changes`：让适配器**一次 dispatch** 应用整批替换，
 * 撤销一步即可回退整批（不整篇重灌，撤销栈口径与 AC-EDITOR-06 一致）。
 */
export function replaceInText(
  text: string,
  matches: readonly InFileMatch[],
  replacement: string,
): { text: string; changes: TextChange[] } {
  const ordered = [...matches].sort((a, b) => b.from - a.from);
  let next = text;
  for (const match of ordered) {
    next = next.slice(0, match.from) + replacement + next.slice(match.to);
  }
  return { text: next, changes: ordered.map((m) => ({ from: m.from, to: m.to, insert: replacement })) };
}
