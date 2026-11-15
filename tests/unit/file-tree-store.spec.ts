import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

vi.mock('@core/ipc/commands', () => ({
  fileTree: vi.fn(),
  preferenceGet: vi.fn(),
  preferenceSet: vi.fn(),
  vaultStateGet: vi.fn(),
  vaultStateSet: vi.fn(),
}));

import {
  fileTree,
  preferenceGet,
  preferenceSet,
  vaultStateGet,
  vaultStateSet,
  type FileNode,
} from '@core/ipc/commands';
import { useFileTreeStore } from '@features/file-tree';
import { STATE_KEY_SHOW_HIDDEN, STATE_KEY_TREE_EXPANDED } from '@features/file-tree';

const mTree = vi.mocked(fileTree);
const mPrefGet = vi.mocked(preferenceGet);
const mPrefSet = vi.mocked(preferenceSet);
const mStateGet = vi.mocked(vaultStateGet);
const mStateSet = vi.mocked(vaultStateSet);

function note(relPath: string): FileNode {
  return { relPath, name: relPath.split('/').pop() ?? relPath, isDir: false, kind: 'note' };
}
function dir(relPath: string, hasChildren = true): FileNode {
  return { relPath, name: relPath, isDir: true, kind: 'other', hasChildren };
}

/** 固定的两层树：dir/a.md、dir/sub/b.md、root.md */
function stubTree(): void {
  mTree.mockImplementation(async (parent?: string) => {
    if (!parent) return [dir('dir'), note('root.md')];
    if (parent === 'dir') return [dir('dir/sub'), note('dir/a.md')];
    if (parent === 'dir/sub') return [note('dir/sub/b.md')];
    return [];
  });
}

describe('useFileTreeStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    // 必须用 resetAllMocks：clearAllMocks 不会清掉上一个用例设置的 mockRejectedValue，
    // 会把「持久化失败」的错误串到后续用例（CI 上真实踩到过）
    vi.resetAllMocks();
    mPrefGet.mockResolvedValue({ values: {} });
    mStateGet.mockResolvedValue({ values: {} });
    mStateSet.mockResolvedValue(undefined);
    mPrefSet.mockResolvedValue(undefined);
    stubTree();
  });

  it('只加载根层，不预取子层（懒加载）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    expect(store.rows.map((r) => r.relPath)).toEqual(['dir', 'root.md']);
    expect(mTree).toHaveBeenCalledTimes(1);
    expect(mTree).toHaveBeenCalledWith(undefined, false);
  });

  it('展开目录时才请求其子节点，并插入正确的 depth', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    expect(mTree).toHaveBeenCalledWith('dir', false);
    expect(store.rows.map((r) => `${r.relPath}@${r.depth}`)).toEqual([
      'dir@0',
      'dir/sub@1',
      'dir/a.md@1',
      'root.md@0',
    ]);
    expect(store.rows[0].expanded).toBe(true);
  });

  it('折叠后子行消失，且不重复请求', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    mTree.mockClear();
    await store.toggle('dir');
    expect(store.rows.map((r) => r.relPath)).toEqual(['dir', 'root.md']);
    expect(mTree).not.toHaveBeenCalled();
  });

  it('展开后立即持久化到 vault_state（AC-FILE-08）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    expect(mStateSet).toHaveBeenCalledWith({ [STATE_KEY_TREE_EXPANDED]: ['dir'] });
  });

  it('restoreState 从**本 Vault 的 vault_state** 恢复隐藏文件开关与展开层级（AC-FILE-08 / FR-FILE-05 / DEBT-15）', async () => {
    // 值放在 vault_state 里 ⇒ 必须被采纳，且**不再读**全局 preference（作用域已收敛）
    mStateGet.mockResolvedValue({
      values: { [STATE_KEY_SHOW_HIDDEN]: true, [STATE_KEY_TREE_EXPANDED]: ['dir/sub'] },
    });
    mPrefGet.mockResolvedValue({ values: { [STATE_KEY_SHOW_HIDDEN]: false } });
    const store = useFileTreeStore();
    await store.restoreState();
    expect(store.includeHidden).toBe(true);
    expect(mPrefGet).not.toHaveBeenCalled();
    expect(mTree).toHaveBeenCalledWith(undefined, true);
    expect(store.rows.map((r) => r.relPath)).toEqual([
      'dir',
      'dir/sub',
      'dir/sub/b.md',
      'dir/a.md',
      'root.md',
    ]);
  });

  it('FR-FILE-02：setSortMode 只重排不重新请求（默认名称；mtime 新的在前）', async () => {
    mTree.mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note', mtimeMs: 100 },
      { relPath: 'b.md', name: 'b.md', isDir: false, kind: 'note', mtimeMs: 900 },
    ]);
    const store = useFileTreeStore();
    expect(store.sortMode).toBe('name');
    await store.loadRoot();
    expect(store.rows.map((r) => r.name)).toEqual(['a.md', 'b.md']);
    mTree.mockClear();
    store.setSortMode('mtime');
    expect(store.sortMode).toBe('mtime');
    expect(store.rows.map((r) => r.name)).toEqual(['b.md', 'a.md']);
    // 纯前端重排：不得再打一次 IPC
    expect(mTree).not.toHaveBeenCalled();
    store.setSortMode('type');
    expect(store.rows.map((r) => r.name)).toEqual(['a.md', 'b.md']);
  });

  it('DEBT-15 一次性兼容：vault_state 没有值时回退旧全局值，并**写回本 Vault**', async () => {
    mStateGet.mockResolvedValue({ values: {} });
    mPrefGet.mockResolvedValue({ values: { [STATE_KEY_SHOW_HIDDEN]: true } });
    const store = useFileTreeStore();
    await store.restoreState();
    expect(store.includeHidden).toBe(true);
    // 写回本 Vault 后，这个值就只属于当前 Vault（不再跟着别的 Vault 串）
    expect(mStateSet).toHaveBeenCalledWith({ [STATE_KEY_SHOW_HIDDEN]: true });
  });

  it('DEBT-15 两个来源都没有时保持默认（关）且不写任何东西', async () => {
    mStateGet.mockResolvedValue({ values: {} });
    mPrefGet.mockResolvedValue({ values: {} });
    const store = useFileTreeStore();
    await store.restoreState();
    expect(store.includeHidden).toBe(false);
    expect(mStateSet).not.toHaveBeenCalledWith({ [STATE_KEY_SHOW_HIDDEN]: true });
    expect(mPrefSet).not.toHaveBeenCalled();
  });

  it('切换显示隐藏文件会重载并写入**本 Vault**（不是全局偏好）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    await store.setIncludeHidden(true);
    expect(mStateSet).toHaveBeenCalledWith({ [STATE_KEY_SHOW_HIDDEN]: true });
    expect(mPrefSet).not.toHaveBeenCalled();
    expect(mTree).toHaveBeenLastCalledWith('dir', true);
    // 只恢复了原先展开的那一层（dir），故只断言其直接子行回来了
    expect(store.rows.map((r) => r.relPath)).toContain('dir/a.md');
  });

  it('加载失败时给出中文提示且不抛异常（R-15 不静默）', async () => {
    mTree.mockRejectedValue({ code: 'E_VAULT_NOT_OPEN', message: '当前没有打开的知识库' });
    const store = useFileTreeStore();
    await store.loadRoot();
    expect(store.error).toBe('当前没有打开的知识库');
    expect(store.rows).toEqual([]);
  });

  it('持久化失败时给出提示而不静默（R-15）', async () => {
    mStateSet.mockRejectedValue({ code: 'E_IO_FAILURE', message: '界面状态过大，已跳过保存（不影响笔记数据）' });
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.persistExpanded();
    expect(store.error).toBe('界面状态过大，已跳过保存（不影响笔记数据）');
  });

  it('展开失败时回滚展开标记', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    mTree.mockRejectedValueOnce({ code: 'E_IO_FAILURE', message: '目录不可读' });
    await store.toggle('dir');
    expect(store.error).toBe('目录不可读');
    expect(store.expanded.has('dir')).toBe(false);
  });
});

describe('useFileTreeStore：切库重置 / 本地同步 / 排序方向（M5 冒烟 5.3–5.6）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    mPrefGet.mockResolvedValue({ values: {} });
    mStateGet.mockResolvedValue({ values: {} });
    mStateSet.mockResolvedValue(undefined);
    mPrefSet.mockResolvedValue(undefined);
    stubTree();
  });

  it('reset：清空展开/缓存/可视行/错误与隐藏开关，且**不调用任何 IPC**（切库串树修复）', async () => {
    mStateGet.mockResolvedValue({ values: { [STATE_KEY_SHOW_HIDDEN]: true, [STATE_KEY_TREE_EXPANDED]: ['dir'] } });
    const store = useFileTreeStore();
    await store.restoreState();
    expect(store.includeHidden).toBe(true);
    expect(store.expanded.has('dir')).toBe(true);
    expect(store.rows.length).toBeGreaterThan(0);
    mTree.mockClear();
    mStateGet.mockClear();
    mStateSet.mockClear();
    store.reset();
    expect(store.rows).toEqual([]);
    expect(store.expanded.size).toBe(0);
    expect(store.includeHidden).toBe(false);
    expect(store.error).toBeNull();
    expect(store.loading).toBe(false);
    // 不写 vault_state、不读——新库的状态必须由随后的 restoreState 从 B 自己那份读
    expect(mTree).not.toHaveBeenCalled();
    expect(mStateGet).not.toHaveBeenCalled();
    expect(mStateSet).not.toHaveBeenCalled();
  });

  it('reset 让上一个库还在飞的列表请求作废（装载代次，不回写旧库目录）', async () => {
    let resolveTree: ((nodes: FileNode[]) => void) | null = null;
    mTree.mockImplementation(
      () => new Promise<FileNode[]>((resolve) => { resolveTree = resolve; }),
    );
    const store = useFileTreeStore();
    const pending = store.loadRoot();
    store.reset();
    expect(resolveTree).not.toBeNull();
    (resolveTree as unknown as (nodes: FileNode[]) => void)([note('旧库.md')]);
    await pending;
    expect(store.rows).toEqual([]);
  });

  it('restoreState 以持久化值为唯一权威：先清空展开状态，不与上一轮并集', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    expect(store.expanded.has('dir')).toBe(true);
    // 持久化里没有展开项（例如新库从未展开过）⇒ 恢复后不能凭空带着上一轮的 dir
    mStateGet.mockResolvedValue({ values: {} });
    await store.restoreState();
    expect(store.expanded.size).toBe(0);
    expect(store.rows.map((r) => r.relPath)).toEqual(['dir', 'root.md']);
  });

  it('排序方向：name 默认升序、mtime 默认降序，toggleSortDirection 纯前端翻转', async () => {
    mTree.mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note', mtimeMs: 100 },
      { relPath: 'b.md', name: 'b.md', isDir: false, kind: 'note', mtimeMs: 900 },
    ]);
    const store = useFileTreeStore();
    await store.loadRoot();
    expect(store.sortDirection).toBe('asc');
    store.setSortMode('mtime');
    expect(store.sortDirection).toBe('desc');
    expect(store.rows.map((r) => r.name)).toEqual(['b.md', 'a.md']);
    mTree.mockClear();
    store.toggleSortDirection();
    expect(store.sortDirection).toBe('asc');
    expect(store.rows.map((r) => r.name)).toEqual(['a.md', 'b.md']);
    expect(mTree).not.toHaveBeenCalled();
  });

  it('nodeAdded：本地插入新笔记、展开父目录并持久化（新建后即时在树里）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    expect(store.rows.map((r) => r.relPath)).not.toContain('dir/新笔记.md');
    await store.nodeAdded('dir/新笔记.md', { mtimeMs: 999 });
    const row = store.rows.find((r) => r.relPath === 'dir/新笔记.md');
    expect(row).toBeDefined();
    expect(row?.depth).toBe(1);
    expect(row?.kind).toBe('note');
    expect(store.expanded.has('dir')).toBe(true);
    expect(mStateSet).toHaveBeenCalledWith({ [STATE_KEY_TREE_EXPANDED]: ['dir'] });
  });

  it('nodeAdded：根层新建直接可见；文件夹标记为 isDir 且无子项', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.nodeAdded('新笔记.md', { mtimeMs: 5 });
    await store.nodeAdded('新文件夹', { isDir: true });
    const noteRow = store.rows.find((r) => r.relPath === '新笔记.md');
    const dirRow = store.rows.find((r) => r.relPath === '新文件夹');
    expect(noteRow?.depth).toBe(0);
    expect(dirRow?.isDir).toBe(true);
    expect(dirRow?.hasChildren).toBe(false);
  });

  it('pathRemoved：立刻从本地树移除（不等索引）；目录连带清展开状态与子层缓存', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    expect(store.rows.map((r) => r.relPath)).toContain('dir/a.md');
    store.pathRemoved('dir/a.md');
    expect(store.rows.map((r) => r.relPath)).not.toContain('dir/a.md');
    store.pathRemoved('dir');
    expect(store.rows.map((r) => r.relPath)).toEqual(['root.md']);
    expect(store.expanded.has('dir')).toBe(false);
  });

  it('pathMoved：本地改名/移动整棵子树（旧路径消失、新路径与展开状态一起换）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    store.pathMoved('dir', 'arch');
    const paths = store.rows.map((r) => r.relPath);
    expect(paths).toContain('arch');
    expect(paths).toContain('arch/sub');
    expect(paths).toContain('arch/a.md');
    expect(paths).not.toContain('dir');
    expect(paths.some((p) => p.startsWith('dir/'))).toBe(false);
    expect(store.expanded.has('arch')).toBe(true);
    expect(store.expanded.has('dir')).toBe(false);
  });

  it('pathMoved：同层重命名（父层缓存顺序按新名称重排）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    store.pathMoved('root.md', 'zzz.md');
    const paths = store.rows.map((r) => r.relPath);
    expect(paths).toContain('zzz.md');
    expect(paths).not.toContain('root.md');
  });
});

describe('useFileTreeStore：外部变更的最小同步（kp://fs/*，M5 冒烟 5.3）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    mPrefGet.mockResolvedValue({ values: {} });
    mStateGet.mockResolvedValue({ values: {} });
    mStateSet.mockResolvedValue(undefined);
    mPrefSet.mockResolvedValue(undefined);
    stubTree();
  });

  it('applyMtime：更新缓存里的 mtime 并按当前排序重排，**零 IPC**', async () => {
    mTree.mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note', mtimeMs: 100 },
      { relPath: 'b.md', name: 'b.md', isDir: false, kind: 'note', mtimeMs: 900 },
    ]);
    const store = useFileTreeStore();
    await store.loadRoot();
    store.setSortMode('mtime');
    expect(store.rows.map((r) => r.name)).toEqual(['b.md', 'a.md']);
    mTree.mockClear();
    // 运行期外部把 a.md 改成更新的时间：事件已带磁盘 mtime，不需要再拉一次树
    store.applyMtime('a.md', 5_000);
    expect(store.rows.map((r) => r.name)).toEqual(['a.md', 'b.md']);
    expect(mTree).not.toHaveBeenCalled();
  });

  it('applyMtime：父层未加载 / 找不到节点 / mtime 未变时是安全空操作', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    expect(() => store.applyMtime('dir/不存在.md', 1)).not.toThrow();
    expect(() => store.applyMtime('dir/a.md', 1)).not.toThrow(); // 父层 dir 未加载
    store.applyMtime('root.md', 123);
    expect(store.rows.map((r) => r.relPath)).toEqual(['dir', 'root.md']);
    expect(() => store.applyMtime('root.md', 123)).not.toThrow(); // 值未变
  });

  it('refreshPath：外部新建后只重拉已加载的父层，新文件立刻出现', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    mTree.mockResolvedValue([dir('dir'), note('root.md'), note('新建.md')]);
    await store.refreshPath('新建.md');
    expect(store.rows.map((r) => r.relPath)).toEqual(['dir', '新建.md', 'root.md']);
  });

  it('refreshPath：父层未加载时不发 IPC（懒加载交给展开时）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    mTree.mockClear();
    await store.refreshPath('dir/sub/深层.md');
    expect(mTree).not.toHaveBeenCalled();
  });

  it('refreshPath：父层重拉失败时给出可读原因（R-15 不静默）', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    mTree.mockRejectedValueOnce({ code: 'E_IO_FAILURE', message: '目录不可读' });
    await store.refreshPath('root.md');
    expect(store.error).toBe('目录不可读');
  });
});
