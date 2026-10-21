import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(async () => ({ valid: true })),
}));
vi.mock('@features/file-tree/stores/fileTree', () => ({ useFileTreeStore: () => ({ refresh: vi.fn() }) }));
// FR-FILE-32：删除确认里的引用数来自既有反链查询（替身，用例按需给值）
vi.mock('@core/ipc/linkCommands', () => ({ linkBacklinks: vi.fn(async () => []) }));
vi.mock('@features/editor', () => ({
  useEditorStore: () => ({ openNote: vi.fn(), closeNote: vi.fn(), followRename: vi.fn() }),
}));

import { linkBacklinks } from '@core/ipc/linkCommands';
import { FileOpsDialog, useFileOpsStore } from '@features/file-tree';

const mBacklinks = vi.mocked(linkBacklinks);

function mountDialog() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const ops = useFileOpsStore();
  const wrapper = mount(FileOpsDialog, { global: { plugins: [pinia] } });
  return { wrapper, ops };
}

describe('FileOpsDialog', () => {
  beforeEach(() => vi.resetAllMocks());

  it('无操作时不渲染', () => {
    const { wrapper } = mountDialog();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(false);
  });

  it('新建笔记：显示名称输入与确定/取消', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openNewNote('dir');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(true);
    expect(wrapper.text()).toContain('新建笔记');
  });

  it('AC-FILE-03：非法名称显示具体原因', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openNewFolder('');
    ops.dialog!.validation = { valid: false, reason: '名称以点号结尾' };
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-validation"]').text()).toBe('名称以点号结尾');
  });

  it('AC-FILE-04：冲突时展示三选项并回传选择', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openNewNote('');
    const resolve = vi.spyOn(ops, 'resolveConflict').mockResolvedValue(undefined);
    ops.dialog!.conflict = true;
    ops.dialog!.error = '文件已存在';
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-conflict"]').text()).toContain('自动备份');
    await wrapper.find('[data-testid="conflict-overwrite"]').trigger('click');
    expect(resolve).toHaveBeenCalledWith('overwrite');
    await wrapper.find('[data-testid="conflict-rename-new"]').trigger('click');
    expect(resolve).toHaveBeenCalledWith('renameNew');
  });

  it('FR-FILE-32 / AC-FILE-11：N>0 时显示「该笔记被 N 处引用」并把按钮改成「仍然删除」', async () => {
    mBacklinks.mockResolvedValue([
      { srcRelPath: 'a.md', srcName: 'a', linkCount: 4, embedCount: 0, items: [] },
      { srcRelPath: 'b.md', srcName: 'b', linkCount: 1, embedCount: 0, items: [] },
    ]);
    const { wrapper, ops } = mountDialog();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(5));
    await wrapper.vm.$nextTick();
    const warn = wrapper.find('[data-testid="delete-references"]');
    expect(warn.exists()).toBe(true);
    // 文案逐字对齐 AC-FILE-11
    expect(warn.text()).toContain('该笔记被 5 处引用，删除后这些链接将变为悬空链接');
    expect(wrapper.find('[data-testid="fileops-submit"]').text()).toBe('仍然删除');
  });

  it('FR-FILE-32：N=0 时**不**出现提示、按钮保持「确定」（不制造噪声）', async () => {
    mBacklinks.mockResolvedValue([]);
    const { wrapper, ops } = mountDialog();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(0));
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="delete-references"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="fileops-submit"]').text()).toBe('确定');
  });

  it('R-15：统计失败时如实提示「无法统计引用」，不伪装成 0 处', async () => {
    mBacklinks.mockRejectedValue({ code: 'E_VAULT_NOT_OPEN', message: '知识库未打开' });
    const { wrapper, ops } = mountDialog();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceError).toBe('知识库未打开'));
    await wrapper.vm.$nextTick();
    const err = wrapper.find('[data-testid="delete-references-error"]');
    expect(err.exists()).toBe(true);
    expect(err.text()).toContain('无法统计引用');
    expect(wrapper.find('[data-testid="delete-references"]').exists()).toBe(false);
  });

  it('DEBT-19：移动到… 显示**目标路径**输入与说明（file_move 的 UI 入口）', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openMove('dir/a.md');
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain('移动到');
    expect(wrapper.find('[data-testid="fileops-move-hint"]').exists()).toBe(true);
    const input = wrapper.find('[data-testid="fileops-move-target"]');
    expect(input.exists()).toBe(true);
    expect((input.element as HTMLInputElement).value).toBe('dir/a.md');
    // 移动用的是路径输入，不是"名称"输入（两者语义不同，不能混用同一个 testid）
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(false);
  });

  it('删除：提示移入回收站（不删磁盘内容）且不显示名称输入', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openDelete('dir/a.md');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-delete-hint"]').text()).toContain('回收站');
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(false);
  });
});
