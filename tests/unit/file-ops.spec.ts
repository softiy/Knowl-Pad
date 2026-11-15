import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileMove: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(),
  vaultCurrent: vi.fn(),
}));

const hoisted = vi.hoisted(() => ({
  nodeAdded: vi.fn(async () => {}),
  pathMoved: vi.fn(),
  pathRemoved: vi.fn(),
  openNote: vi.fn(async () => {}),
  reveal: vi.fn(async () => {}),
  closeNote: vi.fn(),
  closeUnder: vi.fn(),
  followRenameUnder: vi.fn(),
}));

vi.mock('@features/file-tree/stores/fileTree', () => ({
  useFileTreeStore: () => ({
    nodeAdded: hoisted.nodeAdded,
    pathMoved: hoisted.pathMoved,
    pathRemoved: hoisted.pathRemoved,
  }),
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
  vaultCurrent,
} from '@core/ipc/commands';
import { linkBacklinks } from '@core/ipc/linkCommands';
import { useFileOpsStore } from '@features/file-tree';
import { NOTICE_ERROR_MS, NOTICE_SUCCESS_MS } from '@features/file-tree/stores/fileNotice';

const mValidate = vi.mocked(fileValidateName);
const mCreate = vi.mocked(noteCreate);
const mFolder = vi.mocked(folderCreate);
const mRename = vi.mocked(fileRename);
const mMove = vi.mocked(fileMove);
const mDelete = vi.mocked(fileDelete);
const mBacklinks = vi.mocked(linkBacklinks);
const mVaultCurrent = vi.mocked(vaultCurrent);

describe('useFileOpsStore（AC-FILE-03/04/05 + M5 冒烟 5.4/5.6 回归）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    mValidate.mockResolvedValue({ valid: true });
    mCreate.mockResolvedValue({ relPath: 'dir/新笔记.md', newMtime: 1 });
    mFolder.mockResolvedValue(undefined);
    mRename.mockResolvedValue({ from: 'a.md', to: 'b.md', newMtimeMs: 2 });
    mMove.mockResolvedValue({ from: 'dir/a.md', to: 'arch/a.md', newMtimeMs: 3 });
    mDelete.mockResolvedValue({ relPath: 'a.md', trashedCount: 1 });
    // 平台口径默认按 Windows/macOS 的常见情形（不区分大小写）
    mVaultCurrent.mockResolvedValue({
      root: 'C:/vault',
      displayName: 'vault',
      caseInsensitiveFs: true,
    });
  });

  afterEach(() => {
    // 提示自动清除用的是真实定时器，用例结束后必须还原，避免影响其它用例
    vi.useRealTimers();
  });

  it('AC-FILE-03：非法名称给出具体原因且不落盘', async () => {
    mValidate.mockResolvedValue({ valid: false, reason: '不能使用 Windows 保留名 CON' });
    const ops = useFileOpsStore();
    ops.openNewNote('dir');
    ops.dialog!.value = 'CON';
    await ops.submit();
    expect(ops.dialog?.validation?.reason).toBe('不能使用 Windows 保留名 CON');
    expect(mCreate).not.toHaveBeenCalled();
    expect(hoisted.nodeAdded).not.toHaveBeenCalled();
  });

  it('空名称直接拒绝，不调用命令', async () => {
    const ops = useFileOpsStore();
    ops.openNewFolder('');
    ops.dialog!.value = '   ';
    await ops.submit();
    expect(ops.dialog?.validation?.reason).toBe('名称不能为空');
    expect(mFolder).not.toHaveBeenCalled();
  });

  it('新建笔记：按父目录拼路径 → **本地插入树** → 进入编辑态并聚焦标题行（FR-FILE-10）', async () => {
    const ops = useFileOpsStore();
    ops.openNewNote('dir');
    ops.dialog!.value = '新笔记.md';
    await ops.submit();
    expect(mCreate).toHaveBeenCalledWith('dir/新笔记.md', '', 'cancel');
    // M5 冒烟 5.6：索引是异步的，必须本地插入（不能靠"操作后重新拉树"）
    expect(hoisted.nodeAdded).toHaveBeenCalledWith('dir/新笔记.md', { mtimeMs: 1 });
    // FR-FILE-10 的"聚焦标题行"：走既有 reveal 通路（第 1 行），不新造定位机制
    expect(hoisted.reveal).toHaveBeenCalledWith('dir/新笔记.md', 1);
    expect(ops.dialog).toBeNull();
    expect(ops.notice).toContain('已新建');
  });

  it('5.6：新建笔记默认名带 .md；用户手输无扩展名时提交前补 .md', async () => {
    const ops = useFileOpsStore();
    ops.openNewNote('dir');
    expect(ops.dialog?.value).toBe('未命名.md');
    await ops.submit();
    expect(mCreate).toHaveBeenCalledWith('dir/未命名.md', '', 'cancel');

    mCreate.mockResolvedValueOnce({ relPath: 'dir/读书笔记.md', newMtime: 7 });
    ops.openNewNote('dir');
    ops.dialog!.value = '读书笔记';
    await ops.submit();
    expect(mCreate).toHaveBeenLastCalledWith('dir/读书笔记.md', '', 'cancel');
    // 展示给用户的建议也是补过的名字
    expect(hoisted.nodeAdded).toHaveBeenLastCalledWith('dir/读书笔记.md', { mtimeMs: 7 });
  });

  it('FR-EDITOR-27：从悬空链接创建笔记时，建议名同样补 .md', () => {
    const ops = useFileOpsStore();
    ops.createFromLink('被引用三次');
    expect(ops.dialog?.mode).toBe('newNote');
    expect(ops.dialog?.value).toBe('被引用三次.md');
  });

  it('新建文件夹：**不补** .md，并以 isDir 本地插入树', async () => {
    const ops = useFileOpsStore();
    ops.openNewFolder('dir');
    ops.dialog!.value = '资料';
    await ops.submit();
    expect(mFolder).toHaveBeenCalledWith('dir/资料');
    expect(hoisted.nodeAdded).toHaveBeenCalledWith('dir/资料', { isDir: true });
    expect(mCreate).not.toHaveBeenCalled();
  });

  it('DEBT-19：移动到… —— 调 file_move、编辑器跟随、本地树同步、给出提示', async () => {
    const ops = useFileOpsStore();
    ops.openMove('dir/a.md');
    expect(ops.dialog?.mode).toBe('move');
    expect(ops.dialog?.value).toBe('dir/a.md');
    ops.dialog!.value = 'arch/a.md';
    await ops.submit();
    expect(mMove).toHaveBeenCalledWith('dir/a.md', 'arch/a.md', 'cancel');
    expect(hoisted.followRenameUnder).toHaveBeenCalledWith('dir/a.md', 'arch/a.md');
    expect(hoisted.pathMoved).toHaveBeenCalledWith('dir/a.md', 'arch/a.md');
    expect(ops.notice).toContain('已移动到');
    expect(ops.dialog).toBeNull();
  });

  it('5.4：移动到同一路径 → 不发命令、红字「与当前相同」、对话框保持打开', async () => {
    const ops = useFileOpsStore();
    ops.openMove('dir/a.md');
    await ops.submit();
    expect(mMove).not.toHaveBeenCalled();
    expect(ops.dialog?.error).toContain('与当前相同');
    expect(ops.dialog).not.toBeNull();
  });

  it('5.4：目标与当前**大小写不同但同一文件**（Windows 口径）⇒ 同样按"相同"拦截', async () => {
    const ops = useFileOpsStore();
    ops.openMove('归档/待移动.md');
    ops.dialog!.value = '归档/待移动.MD';
    await ops.submit();
    expect(mVaultCurrent).toHaveBeenCalled();
    expect(mMove).not.toHaveBeenCalled();
    expect(ops.dialog?.error).toContain('与当前相同');
  });

  it('5.4：大小写敏感平台上，仅大小写不同的移动是合法改名，应当放行', async () => {
    mVaultCurrent.mockResolvedValue({
      root: '/vault',
      displayName: 'vault',
      caseInsensitiveFs: false,
    });
    mMove.mockResolvedValueOnce({ from: 'a.md', to: 'A.md', newMtimeMs: 4 });
    const ops = useFileOpsStore();
    ops.openMove('a.md');
    ops.dialog!.value = 'A.md';
    await ops.submit();
    expect(mMove).toHaveBeenCalledWith('a.md', 'A.md', 'cancel');
    expect(ops.dialog).toBeNull();
  });

  it('5.4：移动目标的反斜杠会在提交前归一为 /（不把分隔符写进文件名）', async () => {
    const ops = useFileOpsStore();
    ops.openMove('dir/a.md');
    ops.dialog!.value = '归档\\待移动.md';
    await ops.submit();
    expect(mMove).toHaveBeenCalledWith('dir/a.md', '归档/待移动.md', 'cancel');
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

  it('AC-FILE-05：重命名成功后让编辑器跟随 + 本地树改名（路径改新、内容不变）', async () => {
    const ops = useFileOpsStore();
    ops.openRename('dir/a.md');
    ops.dialog!.value = 'b.md';
    await ops.submit();
    expect(mRename).toHaveBeenCalledWith('dir/a.md', 'dir/b.md', 'cancel');
    expect(hoisted.followRenameUnder).toHaveBeenCalledWith('a.md', 'b.md');
    expect(hoisted.pathMoved).toHaveBeenCalledWith('a.md', 'b.md');
  });

  it('删除：递归删除 + 关闭编辑器标签 + 本地移除树节点 + 关闭对话框 + 提示条目数', async () => {
    mDelete.mockResolvedValueOnce({ relPath: 'dir', trashedCount: 3 });
    const ops = useFileOpsStore();
    ops.openDelete('dir');
    await ops.submit();
    expect(mDelete).toHaveBeenCalledWith('dir', true);
    // 删除按**子树**关闭标签（否则子笔记的自动保存会把已删目录写回来）
    expect(hoisted.closeUnder).toHaveBeenCalledWith('dir');
    // M5 冒烟：此前 delete 分支在关对话框之前 return，提示框永远不关
    expect(hoisted.pathRemoved).toHaveBeenCalledWith('dir');
    expect(ops.dialog).toBeNull();
    expect(ops.notice).toContain('3');
  });

  it('删除失败：保持对话框并给出红字原因（不吞错、不假装成功）', async () => {
    mDelete.mockRejectedValueOnce({ code: 'E_PERMISSION_DENIED', message: '没有权限删除' });
    const ops = useFileOpsStore();
    ops.openDelete('a.md');
    await ops.submit();
    expect(ops.dialog).not.toBeNull();
    expect(ops.dialog?.error).toBe('没有权限删除');
    expect(hoisted.pathRemoved).not.toHaveBeenCalled();
    expect(hoisted.closeUnder).not.toHaveBeenCalled();
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
    expect(hoisted.pathRemoved).not.toHaveBeenCalled();
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
      reveal: hoisted.reveal,
      closeNote: hoisted.closeNote,
      followRenameUnder: hoisted.followRenameUnder,
    } as never);
    await ops.submit();
    expect(ops.pendingDelete?.target).toBe('dir');
    expect(mDelete).not.toHaveBeenCalled();

    await ops.resolveDelete('discard');
    expect(mDelete).toHaveBeenCalledWith('dir', true);
    expect(ops.dialog).toBeNull();
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

  it('提示自动清除（M5 冒烟 5.4/5.5）：成功约 4s、错误 8s；新提示覆盖旧提示并重置计时', () => {
    vi.useFakeTimers();
    const ops = useFileOpsStore();
    ops.setNotice('已移动到 归档/待移动.md');
    expect(ops.notice).toContain('已移动到');
    vi.advanceTimersByTime(NOTICE_SUCCESS_MS - 1);
    expect(ops.notice).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(ops.notice).toBeNull();

    ops.setNotice('写盘失败', 'error');
    vi.advanceTimersByTime(NOTICE_SUCCESS_MS);
    expect(ops.notice).toBe('写盘失败');
    vi.advanceTimersByTime(NOTICE_ERROR_MS - NOTICE_SUCCESS_MS);
    expect(ops.notice).toBeNull();

    ops.setNotice('A');
    vi.advanceTimersByTime(NOTICE_SUCCESS_MS - 500);
    ops.setNotice('B');
    vi.advanceTimersByTime(NOTICE_SUCCESS_MS - 500);
    expect(ops.notice).toBe('B');
    vi.advanceTimersByTime(500);
    expect(ops.notice).toBeNull();
  });

  it('clearNotice：切库时立即清掉上一个库的提示（layout 会调用）', () => {
    const ops = useFileOpsStore();
    ops.setNotice('已新建 旧库/某笔记.md');
    ops.clearNotice();
    expect(ops.notice).toBeNull();
  });
});
