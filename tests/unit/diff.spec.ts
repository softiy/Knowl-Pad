import { describe, expect, it } from 'vitest';
import { diffLines } from '@core/utils/diff';

describe('diffLines（FR-EDITOR-34 的差异视图）', () => {
  it('完全相同 → 全为 same', () => {
    const result = diffLines('a\nb', 'a\nb');
    expect(result.truncated).toBe(false);
    expect(result.lines).toEqual([
      { type: 'same', text: 'a' },
      { type: 'same', text: 'b' },
    ]);
  });

  it('尾部新增 → 追加 added（覆盖收尾循环）', () => {
    const result = diffLines('a\nb', 'a\nb\nc');
    expect(result.lines.at(-1)).toEqual({ type: 'added', text: 'c' });
    expect(result.lines.filter((l) => l.type === 'same')).toHaveLength(2);
  });

  it('尾部删除 → 追加 removed（覆盖收尾循环）', () => {
    const result = diffLines('a\nb\nc', 'a');
    expect(result.lines.at(-1)).toEqual({ type: 'removed', text: 'c' });
    expect(result.lines.filter((l) => l.type === 'removed')).toHaveLength(2);
  });

  it('中间改动 → removed + added', () => {
    const result = diffLines('a\nb\nc', 'a\nx\nc');
    expect(result.lines).toEqual([
      { type: 'same', text: 'a' },
      { type: 'removed', text: 'b' },
      { type: 'added', text: 'x' },
      { type: 'same', text: 'c' },
    ]);
  });

  it('空基线 → 全为 added；清空 → 全为 removed', () => {
    expect(diffLines('', 'a\nb').lines.every((l) => l.type === 'added')).toBe(true);
    expect(diffLines('a\nb', '').lines.every((l) => l.type === 'removed')).toBe(true);
  });

  it('超大规模退化为粗粒度（防长任务，§7.5）', () => {
    const before = Array.from({ length: 20 }, (_, i) => `line-${i}`).join('\n');
    const after = Array.from({ length: 20 }, (_, i) => `line-${i + 100}`).join('\n');
    const result = diffLines(before, after, 10);
    expect(result.truncated).toBe(true);
    expect(result.lines.filter((l) => l.type === 'removed')).toHaveLength(20);
    expect(result.lines.filter((l) => l.type === 'added')).toHaveLength(20);
  });
});
