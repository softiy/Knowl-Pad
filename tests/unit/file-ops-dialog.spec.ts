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
vi.mock('@features/editor', () => ({
  useEditorStore: () => ({ openNote: vi.fn(), closeNote: vi.fn(), followRename: vi.fn() }),
}));

import { FileOpsDialog, useFileOpsStore } from '@features/file-tree';

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
