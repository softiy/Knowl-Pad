import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

/**
 * 标签 store（FR-TAG-01~04 / 12 / 13）。
 *
 * 重点不是"调没调到命令"，而是三条**容易写错的口径**：
 * ① 计数**只读不累加**（祖先登记让祖先天然含子孙；相加就重复）；
 * ② 「未分类」用**空串**查询，且它是**合法选中**而不是"没选"；
 * ③ 连点标签时慢的旧响应不能盖掉新的。
 */
const hoisted = vi.hoisted(() => ({
  tree: vi.fn(),
  notes: vi.fn(),
  handlers: [] as (() => void)[],
}));

vi.mock('@core/ipc/tagCommands', () => ({
  tagTree: hoisted.tree,
  tagNotes: hoisted.notes,
}));
vi.mock('@core/ipc/events', () => {
  const register = (h: () => void) => {
    hoisted.handlers.push(h);
    return async () => {};
  };
  return {
    onIndexCompleted: async (h: () => void) => register(h),
    onFsCreated: async (h: () => void) => register(h),
    onFsModified: async (h: () => void) => register(h),
    onFsRemoved: async (h: () => void) => register(h),
    onFsRenamed: async (h: () => void) => register(h),
  };
});

import { useTagsStore } from '@features/tags/stores/tags';
import type { TaggedNote, TagNode } from '@core/ipc/tagCommands';

/** 一棵含「未分类」的小树（计数是**祖先登记后的行数**口径）。 */
function treeFixture(): TagNode[] {
  return [
    {
      norm: '项目', display: '项目', depth: 1, isLeaf: false, refCount: 3, unclassified: false,
      children: [
        { norm: '项目/alpha', display: 'alpha', depth: 2, isLeaf: true, refCount: 2, unclassified: false, children: [] },
        { norm: '项目/beta', display: 'beta', depth: 2, isLeaf: true, refCount: 1, unclassified: false, children: [] },
      ],
    },
    { norm: '随笔', display: '随笔', depth: 1, isLeaf: true, refCount: 9, unclassified: false, children: [] },
    { norm: '', display: '未分类', depth: 0, isLeaf: true, refCount: 4, unclassified: true, children: [] },
  ];
}

describe('标签面板 store', () => {
  // 每个用例收尾都退订：store 的防抖定时器活在实例闭包里，用例之间不清理会互相污染
  // （真实代码里由组件 onUnmounted 调用；这里如实模拟那一步）。
  afterEach(() => {
    useTagsStore().dispose();
    vi.useRealTimers();
  });

  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.handlers.length = 0;
    hoisted.tree.mockReset().mockResolvedValue(treeFixture());
    hoisted.notes.mockReset().mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', mtimeMs: 1 },
    ]);
  });

  it('刷新即向索引库实时查询整棵树（含末尾的「未分类」节点）', async () => {
    const s = useTagsStore();
    await s.refresh();
    expect(hoisted.tree).toHaveBeenCalledTimes(1);
    expect(s.tree).toHaveLength(3);
    expect(s.tree.at(-1)?.unclassified).toBe(true);
    expect(s.loading).toBe(false);
  });

  it('标签计数只读不累加：父节点显示自己的 refCount，不是子孙之和', async () => {
    const s = useTagsStore();
    await s.refresh();
    const parent = s.visibleTree.find((n) => n.norm === '项目');
    // 祖先登记后的行数就是 3；若哪天有人"顺手"改成 3+2+1=6，这条用例会红
    expect(parent?.refCount).toBe(3);
    // 未分类单独计数（篇数口径），不混进"标签个数"：项目 / 项目-alpha / 项目-beta / 随笔 = 4
    expect(s.totalTags).toBe(4);
  });

  it('sortTree 缺省按引用计数降序，但「未分类」恒在最后', async () => {
    const s = useTagsStore();
    await s.refresh();
    // 引用计数：随笔 9 > 项目 3；「未分类」的 4 不参与比较，恒在最后
    expect(s.visibleTree.map((n) => n.norm)).toEqual(['随笔', '项目', '']);
    s.setSortMode('name');
    // 中文的 localeCompare 次序不在这里断言（依赖 ICU 数据），只钉住两条与需求有关的性质：
    // 未分类仍在最后；display 参与排序（不是按 norm —— 'zzz' 会被排到最前就说明用的是 display）。
    s.tree.push({ norm: 'zzz', display: '阿尔法', depth: 1, isLeaf: true, refCount: 0, unclassified: false, children: [] });
    const names = s.visibleTree.map((n) => n.display);
    expect(names[0]).toBe('阿尔法');
    expect(names.at(-1)).toBe('未分类');
  });

  it('按名称排序用 **display** 而不是 norm（FR-TAG-09 的显示名口径）', async () => {
    const s = useTagsStore();
    hoisted.tree.mockResolvedValue([
      // refCount 故意让"显示名靠后"的那个更大：两种排序的结果因此**不同向**（否则用例分不清用的是哪个键）
      { norm: 'work', display: 'Zeta', depth: 1, isLeaf: true, refCount: 5, unclassified: false, children: [] },
      { norm: 'aaa', display: 'Alpha', depth: 1, isLeaf: true, refCount: 1, unclassified: false, children: [] },
    ]);
    await s.refresh();
    s.setSortMode('name');
    // 按 norm 排会是 aaa(Alpha) → work(Zeta)；按 display 排是 Alpha → Zeta，两者不同 ⇒ 能区分
    expect(s.visibleTree.map((n) => n.display)).toEqual(['Alpha', 'Zeta']);
    s.setSortMode('refCount');
    expect(s.visibleTree.map((n) => n.display)).toEqual(['Zeta', 'Alpha']);
  });

  it('过滤按规范键与显示名匹配，并保留祖先链', async () => {
    const s = useTagsStore();
    await s.refresh();
    s.setFilter('alpha');
    expect(s.hasFilter).toBe(true);
    expect(s.rows.map((r) => r.node.norm)).toEqual(['项目', '项目/alpha']);
    s.setFilter('不存在');
    expect(s.rows).toEqual([]);
  });

  it('选中「未分类」用**空串**查询，且它算"已选中"（用真值判定就点不出来）', async () => {
    const s = useTagsStore();
    await s.refresh();
    s.select('');
    expect(s.selectedNorm).toBe('');
    expect(s.selectedNode?.unclassified).toBe(true);
    // 直接把 store 持有的那个值交给查询：它必须是**空串**（而不是 null / undefined）
    await s.loadNotes(s.selectedNorm ?? 'MISSING');
    expect(hoisted.notes).toHaveBeenCalledWith('', false);
  });

  it('切换"含子级"会如实透传并重查（尽管勘误 D-40② 说它当前是 no-op）', async () => {
    const s = useTagsStore();
    await s.refresh();
    s.select('项目');
    await s.loadNotes('项目');
    expect(hoisted.notes).toHaveBeenLastCalledWith('项目', false);
    s.setIncludeChildren(true);
    await Promise.resolve();
    expect(s.includeChildren).toBe(true);
    // UI 不假装它生效：调用方**确实**把 true 交给了后端（"无差别"是后端的祖先登记造成的）
    expect(hoisted.notes).toHaveBeenLastCalledWith('项目', true);
  });

  it('连点标签时丢弃过期响应（慢的旧响应不能盖掉新的）', async () => {
    const s = useTagsStore();
    await s.refresh();
    // 写法说明：`undefined` 初值 + 可选调用。写成 `| null` 再 `?.` 会被 TS 收窄成 `never`
    // （它看不见 Promise executor 里的赋值），得到"不可调用"的假错。
    let releaseFirst: ((v: TaggedNote[]) => void) | undefined;
    hoisted.notes.mockImplementationOnce(
      () => new Promise<TaggedNote[]>((resolve) => { releaseFirst = resolve; }),
    );
    hoisted.notes.mockResolvedValueOnce([{ relPath: 'new.md', name: 'new.md', mtimeMs: 2 }]);
    const slow = s.loadNotes('项目');
    const fast = s.loadNotes('随笔');
    await fast;
    releaseFirst?.([{ relPath: 'stale.md', name: 'stale.md', mtimeMs: 1 }]);
    await slow;
    expect(s.notes.map((n) => n.relPath)).toEqual(['new.md']);
  });

  it('查询失败时给出可见错误而不是抛给渲染层（R-15），并清空列表', async () => {
    hoisted.tree.mockRejectedValue(new Error('boom'));
    const s = useTagsStore();
    await s.refresh();
    expect(s.error).toBeTruthy();
    expect(s.tree).toEqual([]);
    expect(s.loading).toBe(false);
  });

  it('笔记查询失败同样可见（R-15）', async () => {
    const s = useTagsStore();
    hoisted.notes.mockRejectedValue(new Error('notes boom'));
    await s.loadNotes('随笔');
    expect(s.error).toBe('notes boom');
    expect(s.notes).toEqual([]);
  });

  it('事件到来后防抖刷新，且订阅幂等（重复挂载不叠加）', async () => {
    vi.useFakeTimers();
    const s = useTagsStore();
    await s.subscribe();
    await s.subscribe();
    expect(hoisted.handlers.length).toBe(5);
    const before = hoisted.tree.mock.calls.length;
    for (const h of hoisted.handlers) h();
    for (const h of hoisted.handlers) h();
    await vi.advanceTimersByTimeAsync(300);
    expect(hoisted.tree.mock.calls.length).toBe(before + 1);
    s.dispose();
    // dispose 之后事件不再触发查询（不留悬挂订阅）。
    // 用**全新的 store 实例**（同一个 Pinia）：上一个实例可能留了一个已排定但未执行的防抖定时器，
    // 它会在下面这段推进时间时"补跑"一次，把这条断言变成一个与被测行为无关的假红。
    const fresh = useTagsStore();
    const beforeFresh = hoisted.tree.mock.calls.length;
    for (const h of hoisted.handlers) h();
    await vi.advanceTimersByTimeAsync(300);
    // 只有**新实例**的定时器跑了一次：旧实例的已被 dispose 清掉（同一批事件不会打两次 IPC）。
    expect(hoisted.tree.mock.calls.length).toBe(beforeFresh + 1);
    expect(fresh.tree.map((n) => n.norm)).toEqual(['项目', '随笔', '']);
    vi.useRealTimers();
  });

  it('reloadActive 同时刷树与选中项的笔记（改写落地后树要跟着变，AC-TAG-02）', async () => {
    const s = useTagsStore();
    await s.refresh();
    s.select('项目');
    await s.reloadActive();
    expect(hoisted.tree).toHaveBeenCalledTimes(2);
    expect(hoisted.notes).toHaveBeenLastCalledWith('项目', false);
  });

  it('面板开关是 store 状态（快捷键与关闭按钮共用同一个真相）', () => {
    const s = useTagsStore();
    expect(s.panelOpen).toBe(false);
    s.openPanel();
    expect(s.panelOpen).toBe(true);
    s.closePanel();
    expect(s.panelOpen).toBe(false);
  });
});
