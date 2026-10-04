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
import { useFileTreeStore } from '@features/files';
import { PREF_SHOW_HIDDEN, STATE_KEY_TREE_EXPANDED } from '@features/files';

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

  it('restoreState 恢复展开层级与隐藏文件偏好（AC-FILE-08 / FR-FILE-05）', async () => {
    mPrefGet.mockResolvedValue({ values: { [PREF_SHOW_HIDDEN]: true } });
    mStateGet.mockResolvedValue({ values: { [STATE_KEY_TREE_EXPANDED]: ['dir/sub'] } });
    const store = useFileTreeStore();
    await store.restoreState();
    expect(store.includeHidden).toBe(true);
    expect(mTree).toHaveBeenCalledWith(undefined, true);
    expect(store.rows.map((r) => r.relPath)).toEqual([
      'dir',
      'dir/sub',
      'dir/sub/b.md',
      'dir/a.md',
      'root.md',
    ]);
  });

  it('切换显示隐藏文件会重载并写入全局偏好', async () => {
    const store = useFileTreeStore();
    await store.loadRoot();
    await store.toggle('dir');
    await store.setIncludeHidden(true);
    expect(mPrefSet).toHaveBeenCalledWith({ [PREF_SHOW_HIDDEN]: true });
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
