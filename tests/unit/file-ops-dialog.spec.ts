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

  it('删除：提示移入回收站（不删磁盘内容）且不显示名称输入', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openDelete('dir/a.md');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-delete-hint"]').text()).toContain('回收站');
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(false);
  });
});
