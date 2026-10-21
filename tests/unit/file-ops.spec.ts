import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileMove: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(),
}));

const hoisted = vi.hoisted(() => ({
  refresh: vi.fn(async () => {}),
  openNote: vi.fn(async () => {}),
  reveal: vi.fn(async () => {}),
  closeNote: vi.fn(),
  closeUnder: vi.fn(),
  followRenameUnder: vi.fn(),
}));

vi.mock('@features/file-tree/stores/fileTree', () => ({
  useFileTreeStore: () => ({ refresh: hoisted.refresh }),
}));

// FR-FILE-32：删除前的引用统计走既有反链查询（此处替身，用例按需给值）
vi.mock('@core/ipc/linkCommands', () => ({ linkBacklinks: vi.fn(async () => []) }));

vi.mock('@features/editor', () => ({
  useEditorStore: () => ({
    buffers: [],
    openNote: hoisted.openNote,
    reveal: hoisted.reveal,
    closeNote: hoisted.closeNote,
    closeUnder: hoisted.closeUnder,
    followRenameUnder: hoisted.followRenameUnder,
    dirtyAmong: () => [],
  }),
}));

import {
  fileDelete,
  fileMove,
  fileRename,
  fileValidateName,
  folderCreate,
  noteCreate,
} from '@core/ipc/commands';
import { linkBacklinks } from '@core/ipc/linkCommands';
import { useFileOpsStore } from '@features/file-tree';

const mValidate = vi.mocked(fileValidateName);
const mCreate = vi.mocked(noteCreate);
const mFolder = vi.mocked(folderCreate);
const mRename = vi.mocked(fileRename);
const mMove = vi.mocked(fileMove);
const mDelete = vi.mocked(fileDelete);
const mBacklinks = vi.mocked(linkBacklinks);

describe('useFileOpsStore（AC-FILE-03/04/05）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    mValidate.mockResolvedValue({ valid: true });
    mCreate.mockResolvedValue({ relPath: 'dir/新笔记.md', newMtime: 1 });
    mFolder.mockResolvedValue(undefined);
    mRename.mockResolvedValue({ from: 'a.md', to: 'b.md', newMtimeMs: 2 });
    mMove.mockResolvedValue({ from: 'dir/a.md', to: 'arch/a.md', newMtimeMs: 3 });
    mDelete.mockResolvedValue({ relPath: 'a.md', trashedCount: 1 });
  });

  it('AC-FILE-03：非法名称给出具体原因且不落盘', async () => {
    mValidate.mockResolvedValue({ valid: false, reason: '不能使用 Windows 保留名 CON' });
    const ops = useFileOpsStore();
    ops.openNewNote('dir');
    ops.dialog!.value = 'CON';
    await ops.submit();
    expect(ops.dialog?.validation?.reason).toBe('不能使用 Windows 保留名 CON');
    expect(mCreate).not.toHaveBeenCalled();
    expect(hoisted.refresh).not.toHaveBeenCalled();
  });

  it('空名称直接拒绝，不调用命令', async () => {
    const ops = useFileOpsStore();
    ops.openNewFolder('');
    ops.dialog!.value = '   ';
    await ops.submit();
    expect(ops.dialog?.validation?.reason).toBe('名称不能为空');
    expect(mFolder).not.toHaveBeenCalled();
  });

  it('新建笔记：按父目录拼路径 → 刷新树 → 进入编辑态并**聚焦标题行**（FR-FILE-10）', async () => {
    const ops = useFileOpsStore();
    ops.openNewNote('dir');
    ops.dialog!.value = '新笔记.md';
    await ops.submit();
    expect(mCreate).toHaveBeenCalledWith('dir/新笔记.md', '', 'cancel');
    expect(hoisted.refresh).toHaveBeenCalledTimes(1);
    // FR-FILE-10 的"聚焦标题行"：走既有 reveal 通路（第 1 行），不新造定位机制
    expect(hoisted.reveal).toHaveBeenCalledWith('dir/新笔记.md', 1);
    expect(ops.dialog).toBeNull();
    expect(ops.notice).toContain('已新建');
  });

  it('DEBT-19：移动到… —— 调 file_move、编辑器跟随、刷新树、给出提示', async () => {
    const ops = useFileOpsStore();
    ops.openMove('dir/a.md');
    expect(ops.dialog?.mode).toBe('move');
    expect(ops.dialog?.value).toBe('dir/a.md');
    ops.dialog!.value = 'arch/a.md';
    await ops.submit();
    expect(mMove).toHaveBeenCalledWith('dir/a.md', 'arch/a.md', 'cancel');
    expect(hoisted.followRenameUnder).toHaveBeenCalledWith('dir/a.md', 'arch/a.md');
    expect(hoisted.refresh).toHaveBeenCalledTimes(1);
    expect(ops.notice).toContain('已移动到');
    expect(ops.dialog).toBeNull();
  });

  it('DEBT-19：移动到同一路径 → 不发命令、给出可读原因', async () => {
    const ops = useFileOpsStore();
    ops.openMove('dir/a.md');
    await ops.submit();
    expect(mMove).not.toHaveBeenCalled();
    expect(ops.dialog?.error).toContain('相同');
  });

  it('AC-FILE-04：目标已存在 → 进入冲突态；选「覆盖」后带策略重试', async () => {
    mCreate.mockRejectedValueOnce({ code: 'E_FILE_EXISTS', message: '文件已存在' });
    const ops = useFileOpsStore();
    ops.openNewNote('');
    ops.dialog!.value = 'note.md';
    await ops.submit();
    expect(ops.dialog?.conflict).toBe(true);
    expect(ops.dialog?.error).toBe('文件已存在');
    expect(mCreate).toHaveBeenCalledTimes(1);

    await ops.resolveConflict('overwrite');
    expect(mCreate).toHaveBeenLastCalledWith('note.md', '', 'overwrite');
    expect(ops.dialog).toBeNull();
  });

  it('AC-FILE-04：冲突时选「取消」不重试', async () => {
    mCreate.mockRejectedValueOnce({ code: 'E_FILE_EXISTS', message: '文件已存在' });
    const ops = useFileOpsStore();
    ops.openNewNote('');
    ops.dialog!.value = 'note.md';
    await ops.submit();
    await ops.resolveConflict('cancel');
    expect(mCreate).toHaveBeenCalledTimes(1);
    expect(ops.dialog?.conflict).toBe(false);
  });

  it('AC-FILE-05：重命名成功后让编辑器跟随（路径改新、内容不变）', async () => {
    const ops = useFileOpsStore();
    ops.openRename('dir/a.md');
    ops.dialog!.value = 'b.md';
    await ops.submit();
    expect(mRename).toHaveBeenCalledWith('dir/a.md', 'dir/b.md', 'cancel');
    expect(hoisted.followRenameUnder).toHaveBeenCalledWith('a.md', 'b.md');
    expect(hoisted.refresh).toHaveBeenCalledTimes(1);
  });

  it('删除：递归删除 + 关闭编辑器标签 + 提示回收站条目数', async () => {
    mDelete.mockResolvedValueOnce({ relPath: 'dir', trashedCount: 3 });
    const ops = useFileOpsStore();
    ops.openDelete('dir');
    await ops.submit();
    expect(mDelete).toHaveBeenCalledWith('dir', true);
    // 删除按**子树**关闭标签（否则子笔记的自动保存会把已删目录写回来）
    expect(hoisted.closeUnder).toHaveBeenCalledWith('dir');
    expect(ops.notice).toContain('3');
  });

  it('FR-FILE-32 / AC-FILE-11：删除前用**既有反链查询**统计引用处数（N>0 才能确认删除）', async () => {
    mBacklinks.mockResolvedValue([
      { srcRelPath: 'a.md', srcName: 'a', linkCount: 3, embedCount: 0, items: [] },
      { srcRelPath: 'b.md', srcName: 'b', linkCount: 1, embedCount: 1, items: [] },
    ]);
    const ops = useFileOpsStore();
    ops.openDelete('dir/A.md');
    // 对话框先出现、计数随后填进来（异步，不阻塞删除）
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(5));
    expect(mBacklinks).toHaveBeenCalledWith('dir/A.md', true);
    expect(mDelete).not.toHaveBeenCalled();
    await ops.submit();
    expect(mDelete).toHaveBeenCalledWith('dir/A.md', true);
    // AC-FILE-11：删除**不改写引用方文件** —— 除了软删除，没有任何写命令被触发
    expect(mCreate).not.toHaveBeenCalled();
    expect(mRename).not.toHaveBeenCalled();
    expect(mMove).not.toHaveBeenCalled();
  });

  it('FR-FILE-32：N=0 时 referenceCount 为 0（界面据此不提示）', async () => {
    mBacklinks.mockResolvedValue([]);
    const ops = useFileOpsStore();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(0));
    expect(ops.dialog?.referenceError).toBeNull();
  });

  it('FR-FILE-32：取消删除 ⇒ 不做任何删除动作', async () => {
    mBacklinks.mockResolvedValue([{ srcRelPath: 'a.md', srcName: 'a', linkCount: 2, embedCount: 0, items: [] }]);
    const ops = useFileOpsStore();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(2));
    ops.close();
    expect(mDelete).not.toHaveBeenCalled();
    expect(hoisted.refresh).not.toHaveBeenCalled();
  });

  it('R-15：引用统计失败不阻断删除，但必须留下可读原因', async () => {
    mBacklinks.mockRejectedValue({ code: 'E_VAULT_NOT_OPEN', message: '知识库未打开' });
    const ops = useFileOpsStore();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceError).toBe('知识库未打开'));
    expect(ops.dialog?.referenceCount).toBeNull(); // 不用 0 冒充失败
    await ops.submit();
    expect(mDelete).toHaveBeenCalledWith('A.md', true);
  });

  it('R-07：删除含未保存标签的路径前必须先确认', async () => {
    const ops = useFileOpsStore();
    ops.openDelete('dir');
    // 替身默认 buffers 为空；这里模拟「该路径下有脏标签」
    const editorMod = await import('@features/editor');
    const spy = vi.spyOn(editorMod, 'useEditorStore').mockReturnValue({
      buffers: [{ relPath: 'dir/a.md' }],
      dirtyAmong: () => ['dir/a.md'],
      closeUnder: hoisted.closeUnder,
      save: vi.fn(async () => {}),
      openNote: hoisted.openNote,
      closeNote: hoisted.closeNote,
      followRenameUnder: hoisted.followRenameUnder,
    } as never);
    await ops.submit();
    expect(ops.pendingDelete?.target).toBe('dir');
    expect(mDelete).not.toHaveBeenCalled();

    await ops.resolveDelete('discard');
    expect(mDelete).toHaveBeenCalledWith('dir', true);
    spy.mockRestore();
  });
  it('其它错误只提示不进入冲突态', async () => {
    mFolder.mockRejectedValueOnce({ code: 'E_PERMISSION_DENIED', message: '没有权限创建文件夹' });
    const ops = useFileOpsStore();
    ops.openNewFolder('dir');
    ops.dialog!.value = 'sub';
    await ops.submit();
    expect(ops.dialog?.error).toBe('没有权限创建文件夹');
    expect(ops.dialog?.conflict).toBe(false);
    expect(ops.dialog).not.toBeNull();
  });
});
