import { describe, expect, it } from 'vitest';
import {
  DEFAULT_TAG_SORT,
  compareTagNodes,
  filterTree,
  findTagNode,
  flattenTree,
  formatNoteTime,
  isSelectedNode,
  matchesFilter,
  refCountLabel,
  sortTree,
  subtreeMatches,
  walkTree,
} from '@features/tags/logic';
import type { TagNode } from '@core/ipc/tagCommands';

/** 造一个节点（缺省是真标签，不是「未分类」）。 */
function node(over: Partial<TagNode> & { norm: string }): TagNode {
  return {
    display: over.norm,
    depth: over.norm.split('/').length,
    isLeaf: true,
    refCount: 0,
    unclassified: false,
    children: [],
    ...over,
  };
}

/** AC-TAG-01 的树：项目 → alpha → {前端, 后端}、项目 → beta、随笔；计数是**祖先登记后的行数**。 */
function acTag01Tree(): TagNode[] {
  return [
    node({
      norm: '项目',
      display: '项目',
      depth: 1,
      isLeaf: false,
      refCount: 4,
      children: [
        node({
          norm: '项目/alpha',
          display: 'alpha',
          depth: 2,
          isLeaf: false,
          refCount: 3,
          children: [
            node({ norm: '项目/alpha/前端', display: '前端', depth: 3, refCount: 2 }),
            node({ norm: '项目/alpha/后端', display: '后端', depth: 3, refCount: 1 }),
          ],
        }),
        node({ norm: '项目/beta', display: 'beta', depth: 2, refCount: 1 }),
      ],
    }),
    node({ norm: '随笔', display: '随笔', depth: 1, refCount: 5 }),
  ];
}

describe('标签树纯逻辑：排序（FR-TAG-04）', () => {
  it('缺省按引用计数降序（PRD §3.3 的标签面板查询口径）', () => {
    expect(DEFAULT_TAG_SORT).toBe('refCount');
    const sorted = sortTree([node({ norm: 'a', refCount: 1 }), node({ norm: 'b', refCount: 9 })]);
    expect(sorted.map((n) => n.norm)).toEqual(['b', 'a']);
  });

  it('按名称排序时用 localeCompare（中文不能靠码点）', () => {
    const sorted = sortTree(
      [node({ norm: 'z', display: '随笔' }), node({ norm: 'a', display: '笔记' })],
      'name',
    );
    expect(sorted.map((n) => n.norm)).toEqual(['a', 'z']);
  });

  it('「未分类」**恒排最后**，压过排序方式（PRD §5.3.6.1 明定"排在全部标签之后"）', () => {
    const unclassified = node({ norm: '', display: '未分类', depth: 0, refCount: 999, unclassified: true });
    const tags = [node({ norm: 'a', refCount: 1 })];
    // 按引用计数时它的 999 是最大值、按名称时"未分类"也不该插在中间
    expect(sortTree([...tags, unclassified], 'refCount').at(-1)?.norm).toBe('');
    expect(sortTree([unclassified, ...tags], 'name').at(-1)?.unclassified).toBe(true);
  });

  it('比较函数不累加子孙计数（祖先登记已含子孙，相加就是重复计数）', () => {
    const parent = node({ norm: 'p', depth: 1, isLeaf: false, refCount: 3, children: [
      node({ norm: 'p/x', depth: 2, refCount: 2 }),
      node({ norm: 'p/y', depth: 2, refCount: 1 }),
    ] });
    // 3（父自身行数，已含两个子）—— 不是 3+2+1=6
    expect(parent.refCount).toBe(3);
    // 与"没有子孙、计数同为 3"的节点**打平**：比较函数只看 refCount 本身，
    // 若它偷偷把子孙加起来（3+2+1=6）这里会返回"父 > 兄弟"，用例就会红。
    const plain = node({ norm: 'p2', display: 'p', refCount: 3 });
    expect(compareTagNodes(parent, plain, 'refCount')).toBe(0);
    // 反向对照：真的更大才排前面（证明上一条不是因为比较函数恒返回 0）
    expect(compareTagNodes(parent, node({ norm: 'q', display: 'q', refCount: 2 }), 'refCount')).toBeLessThan(0);
  });

  it('排序返回副本，不改动入参（store 里的树是共享状态）', () => {
    const tree = [node({ norm: 'a', refCount: 1 }), node({ norm: 'b', refCount: 2 })];
    const sorted = sortTree(tree);
    expect(sorted.map((n) => n.norm)).toEqual(['b', 'a']);
    expect(tree.map((n) => n.norm)).toEqual(['a', 'b']);
    expect(sorted[0]).not.toBe(tree[0]);
  });
});

describe('标签树纯逻辑：过滤（FR-TAG-04）', () => {
  it('空串 / 全空白 ⇒ 原样返回（不过滤）', () => {
    const tree = acTag01Tree();
    expect(filterTree(tree, '')).toBe(tree);
    expect(filterTree(tree, '   ')).toBe(tree);
  });

  it('命中子孙时**保留全部祖先链**（否则树被截断成孤儿子节点）', () => {
    const kept = filterTree(acTag01Tree(), '前端');
    expect(kept.map((n) => n.norm)).toEqual(['项目']);
    expect(kept[0]?.children.map((n) => n.norm)).toEqual(['项目/alpha']);
    expect(kept[0]?.children[0]?.children.map((n) => n.norm)).toEqual(['项目/alpha/前端']);
  });

  it('大小写不敏感，且同时匹配规范键与显示名', () => {
    const work = node({ norm: 'work', display: 'Work' });
    expect(matchesFilter(work, 'WORK')).toBe(true);
    expect(matchesFilter(work, 'work')).toBe(true);
    expect(subtreeMatches(work, 'ork')).toBe(true);
    expect(matchesFilter(work, 'other')).toBe(false);
  });

  it('完全没命中 ⇒ 空数组（面板据此区分"没有标签"与"过滤没命中"）', () => {
    expect(filterTree(acTag01Tree(), '不存在的标签')).toEqual([]);
  });

  it('过滤不破坏层级：保留下来的节点的 depth 仍与原名一致', () => {
    const kept = filterTree(acTag01Tree(), '后端');
    const rows = flattenTree(kept);
    expect(rows.map((r) => r.depth)).toEqual([1, 2, 3]);
    expect(rows.at(-1)?.node.norm).toBe('项目/alpha/后端');
  });
});

describe('标签树纯逻辑：扁平化与查找', () => {
  it('前序遍历：父先于子，深度随层级递增（FR-TAG-02 的缩进依据）', () => {
    const rows = flattenTree(acTag01Tree());
    expect(rows.map((r) => r.node.norm)).toEqual([
      '项目', '项目/alpha', '项目/alpha/前端', '项目/alpha/后端', '项目/beta', '随笔',
    ]);
    expect(rows.map((r) => r.depth)).toEqual([1, 2, 3, 3, 2, 1]);
    expect(rows[0]?.hasChildren).toBe(true);
    expect(rows[2]?.hasChildren).toBe(false);
  });

  it('行键用 norm（整棵树唯一）；「未分类」的空 norm 由 display 兜底', () => {
    const rows = flattenTree([
      node({ norm: 'a' }),
      node({ norm: '', display: '未分类', unclassified: true, depth: 0 }),
    ]);
    expect(rows.map((r) => r.key)).toEqual(['a', 'unclassified:未分类']);
  });

  it('walkTree 覆盖全部节点（含深层）', () => {
    expect(walkTree(acTag01Tree()).map((n) => n.norm)).toContain('项目/alpha/前端');
    expect(walkTree(acTag01Tree())).toHaveLength(6);
  });

  it('按 norm 找节点；「未分类」用空串；找不到返回 null', () => {
    const tree = [...acTag01Tree(), node({ norm: '', display: '未分类', unclassified: true, depth: 0 })];
    expect(findTagNode(tree, '项目/beta')?.display).toBe('beta');
    expect(findTagNode(tree, '')?.unclassified).toBe(true);
    expect(findTagNode(tree, 'nope')).toBeNull();
  });
});

describe('标签树纯逻辑：选中与计数文案', () => {
  it('选中按 norm；「未分类」的空串是**合法选中**而不是"没选"', () => {
    const unclassified = node({ norm: '', display: '未分类', unclassified: true, depth: 0 });
    expect(isSelectedNode(unclassified, '')).toBe(true);
    expect(isSelectedNode(node({ norm: 'a' }), '')).toBe(false);
    expect(isSelectedNode(node({ norm: 'a' }), null)).toBe(false);
  });

  it('计数单位：标签是**行数**（次），「未分类」是**篇数**（PRD §5.3.6.1 的计数口径④）', () => {
    expect(refCountLabel(node({ norm: 'a', refCount: 7 }))).toBe('7 次');
    expect(refCountLabel(node({ norm: '', display: '未分类', unclassified: true, refCount: 7 }))).toBe('7 篇');
  });

  it('修改时间：正常值格式化到分钟；未知/非法值给占位符而不是伪造时间', () => {
    expect(formatNoteTime(new Date(2026, 9, 16, 9, 5).getTime())).toBe('2026-10-16 09:05');
    expect(formatNoteTime(0)).toBe('时间未知');
    expect(formatNoteTime(Number.NaN)).toBe('时间未知');
  });
});
