import { describe, expect, it } from 'vitest';
import { KpError, E_SEARCH_SYNTAX } from '@core/ipc/errors';
import type { SearchResultItem } from '@core/ipc/searchCommands';
import {
  EMPTY_HINTS,
  SEARCH_DEBOUNCE_MS,
  SORT_OPTIONS,
  classifySearchError,
  createLatestGuard,
  formatMtime,
  hitCountLabel,
  nextIndex,
  splitHighlight,
  visibleSnippets,
} from '@features/search/logic';

/** 造一条搜索结果（只填本文件用得到的字段）。 */
function item(snippets: number, hitCount: number): SearchResultItem {
  return {
    relPath: 'work/report.md',
    name: 'report.md',
    mtimeMs: 1_700_000_000_000,
    score: -1,
    hitCount,
    snippets: Array.from({ length: snippets }, (_, i) => ({
      lines: [{ line: i + 1, text: 'x', isHitLine: true }],
      highlightLine: i + 1,
      highlightStart: 0,
      highlightEnd: 1,
    })),
  };
}

describe('搜索：错误分类（两种情形必须区分呈现）', () => {
  it('语法错误 ⇒ syntax（附示例）', () => {
    const err = new KpError({ code: E_SEARCH_SYNTAX, message: '引号未闭合' });
    const c = classifySearchError(err);
    expect(c.kind).toBe('syntax');
    expect(c.title).toBe('语法提示');
    expect(c.message).toContain('引号未闭合');
    expect(c.message).toContain('示例');
  });

  it('候选过多（同一个错误码）⇒ too-broad，且**原样透出后端文案里的数字**', () => {
    const err = new KpError({
      code: E_SEARCH_SYNTAX,
      message: 'line: 的候选笔记过多（当前 1234 条、上限 300 条），请先缩小范围',
    });
    const c = classifySearchError(err);
    expect(c.kind).toBe('too-broad');
    expect(c.title).toBe('结果范围太大');
    expect(c.message).toContain('当前 1234 条');
    expect(c.message).toContain('上限 300 条');
    expect(c.message).not.toContain('示例'); // 不是语法提示，不该混入语法示例
  });

  it('其它错误 ⇒ other（既不加示例也不改标题）', () => {
    const c = classifySearchError(new Error('磁盘炸了'));
    expect(c.kind).toBe('other');
    expect(c.title).toBe('搜索失败');
    expect(c.message).toBe('磁盘炸了');
  });
});

describe('搜索：只认最后一次请求', () => {
  it('后发的号才是最新的，旧号一律作废', () => {
    const guard = createLatestGuard();
    const first = guard.next();
    expect(guard.isCurrent(first)).toBe(true);
    const second = guard.next();
    expect(guard.isCurrent(first)).toBe(false);
    expect(guard.isCurrent(second)).toBe(true);
  });
});

describe('搜索：键盘选中项', () => {
  it('上下移动且夹紧在两端（不环绕）', () => {
    expect(nextIndex(-1, 3, 1)).toBe(0);
    expect(nextIndex(0, 3, 1)).toBe(1);
    expect(nextIndex(2, 3, 1)).toBe(2);
    expect(nextIndex(0, 3, -1)).toBe(0);
    expect(nextIndex(-1, 3, -1)).toBe(2);
  });

  it('空列表 ⇒ -1（没有可选项）', () => {
    expect(nextIndex(0, 0, 1)).toBe(-1);
    expect(nextIndex(-1, 0, -1)).toBe(-1);
  });
});

describe('搜索：片段展开与计数', () => {
  it('默认只显示第一条片段，展开后显示全部', () => {
    const it0 = item(3, 5);
    expect(visibleSnippets(it0, false)).toHaveLength(1);
    expect(visibleSnippets(it0, true)).toHaveLength(3);
  });

  it('命中次数文案', () => {
    expect(hitCountLabel(item(1, 5))).toBe('5 处命中');
    expect(hitCountLabel(item(1, 1))).toBe('1 处命中');
  });
});

describe('搜索：高亮切分与时间', () => {
  it('按字符偏移切成三段，越界自动夹紧', () => {
    expect(splitHighlight('abcdef', 2, 4)).toEqual({ before: 'ab', hit: 'cd', after: 'ef' });
    expect(splitHighlight('abc', 9, 12)).toEqual({ before: 'abc', hit: '', after: '' });
    expect(splitHighlight('abc', -3, 1)).toEqual({ before: '', hit: 'a', after: 'bc' });
  });

  it('时间未知时给占位符，不伪造时间', () => {
    expect(formatMtime(0)).toBe('时间未知');
    expect(formatMtime(Number.NaN)).toBe('时间未知');
    expect(formatMtime(Date.UTC(2023, 10, 14, 22, 13))).toMatch(/^2023-11-1[45] \d{2}:\d{2}$/);
  });
});

describe('搜索：常量', () => {
  it('防抖窗口与排序项与需求一致', () => {
    expect(SEARCH_DEBOUNCE_MS).toBe(200); // FR-SEARCH-08
    expect(SORT_OPTIONS.map((o) => o.value)).toEqual(['relevance', 'mtime', 'name', 'path']); // FR-SEARCH-06
    expect(SORT_OPTIONS[0]?.label).toBe('相关度');
    expect(EMPTY_HINTS.length).toBeGreaterThan(0); // FR-SEARCH-11
  });
});
