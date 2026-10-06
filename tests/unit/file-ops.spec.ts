import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(),
}));

const hoisted = vi.hoisted(() => ({
  refresh: vi.fn(async () => {}),
  openNote: vi.fn(async () => {}),
  closeNote: vi.fn(),
  closeUnder: vi.fn(),
  followRename: vi.fn(),
}));

vi.mock('@features/file-tree/stores/fileTree', () => ({
  useFileTreeStore: () => ({ refresh: hoisted.refresh }),
}));

vi.mock('@features/editor', () => ({
  useEditorStore: () => ({
    buffers: [],
    openNote: hoisted.openNote,
    closeNote: hoisted.closeNote,
    closeUnder: hoisted.closeUnder,
    followRename: hoisted.followRename,
    dirtyAmong: () => [],
  }),
}));

import {
  fileDelete,
  fileRename,
  fileValidateName,
  folderCreate,
  noteCreate,
} from '@core/ipc/commands';
import { useFileOpsStore } from '@features/file-tree';

const mValidate = vi.mocked(fileValidateName);
const mCreate = vi.mocked(noteCreate);
const mFolder = vi.mocked(folderCreate);
const mRename = vi.mocked(fileRename);
const mDelete = vi.mocked(fileDelete);

describe('useFileOpsStore（AC-FILE-03/04/05）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    mValidate.mockResolvedValue({ valid: true });
    mCreate.mockResolvedValue({ relPath: 'dir/新笔记.md', newMtime: 1 });
    mFolder.mockResolvedValue(undefined);
    mRename.mockResolvedValue({ from: 'a.md', to: 'b.md', newMtimeMs: 2 });
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

  it('新建笔记：按父目录拼路径 → 刷新树 → 打开编辑器（FR-FILE-10）', async () => {
    const ops = useFileOpsStore();
    ops.openNewNote('dir');
    ops.dialog!.value = '新笔记.md';
    await ops.submit();
    expect(mCreate).toHaveBeenCalledWith('dir/新笔记.md', '', 'cancel');
    expect(hoisted.refresh).toHaveBeenCalledTimes(1);
    expect(hoisted.openNote).toHaveBeenCalledWith('dir/新笔记.md');
    expect(ops.dialog).toBeNull();
    expect(ops.notice).toContain('已新建');
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
    expect(hoisted.followRename).toHaveBeenCalledWith('a.md', 'b.md');
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
      followRename: hoisted.followRename,
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
