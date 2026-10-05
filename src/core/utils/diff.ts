/**
 * 行级差异（LCS 动态规划）。用于 FR-EDITOR-34 的「查看差异」：展示外部版本与我这一版的区别。
 *
 * 规模保护：单元数 `n*m` 超过 `maxCells` 时退化为「整段删除 + 整段新增」并置 `truncated`，
 * 避免超大文件把主线程拖住（§7.5 长任务预算）。
 */

export interface DiffLine {
  type: 'same' | 'added' | 'removed';
  text: string;
}

export interface DiffResult {
  lines: DiffLine[];
  /** 是否因规模过大退化为粗粒度结果 */
  truncated: boolean;
}

const DEFAULT_MAX_CELLS = 4_000_000;

export function diffLines(
  before: string,
  after: string,
  maxCells: number = DEFAULT_MAX_CELLS,
): DiffResult {
  // 空文件是 **0 行**，不是「一个空行」（否则空基线会多出一条 same）
  const a = before === '' ? [] : before.split('\n');
  const b = after === '' ? [] : after.split('\n');
  if (a.length * b.length > maxCells) {
    return {
      truncated: true,
      lines: [
        ...a.map((text) => ({ type: 'removed' as const, text })),
        ...b.map((text) => ({ type: 'added' as const, text })),
      ],
    };
  }
  const n = a.length;
  const m = b.length;
  // dp[i][j] = a[i..] 与 b[j..] 的最长公共子序列长度
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const lines: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      lines.push({ type: 'same', text: a[i] });
      i += 1;
      j += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      lines.push({ type: 'removed', text: a[i] });
      i += 1;
    } else {
      lines.push({ type: 'added', text: b[j] });
      j += 1;
    }
  }
  while (i < n) {
    lines.push({ type: 'removed', text: a[i] });
    i += 1;
  }
  while (j < m) {
    lines.push({ type: 'added', text: b[j] });
    j += 1;
  }
  return { lines, truncated: false };
}
