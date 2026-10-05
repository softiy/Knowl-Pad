import { mount, flushPromises } from '@vue/test-utils';
import { defineComponent } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  vaultCurrent: vi.fn(),
  fileTree: vi.fn(async () => []),
  noteRead: vi.fn(),
  noteWrite: vi.fn(),
  preferenceGet: vi.fn(async () => ({ values: {} })),
  preferenceSet: vi.fn(),
  vaultStateGet: vi.fn(async () => ({ values: {} })),
  vaultStateSet: vi.fn(),
  fileReveal: vi.fn(),
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(async () => ({ valid: true })),
}));

import { vaultCurrent } from '@core/ipc/commands';
import WorkspaceLayout from '@/app/layout/WorkspaceLayout.vue';

const FileTreeStub = defineComponent({
  name: 'FileTree',
  emits: ['open', 'action'],
  template: '<div data-testid="tree-stub" />',
});

function mountLayout() {
  const pinia = createPinia();
  setActivePinia(pinia);
  return mount(WorkspaceLayout, {
    global: { plugins: [pinia], stubs: { FileTree: FileTreeStub, EditorView: true, VaultList: true } },
  });
}

describe('WorkspaceLayout（M2 接线）', () => {
  beforeEach(() => vi.resetAllMocks());

  it('未打开 Vault 时展示 Vault 列表', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue(null);
    const wrapper = mountLayout();
    await flushPromises();
    expect(wrapper.find('[data-testid="tree-stub"]').exists()).toBe(false);
    expect(wrapper.findComponent({ name: 'VaultList' }).exists() || wrapper.html().includes('vault-list')).toBe(true);
  });

  it('已打开 Vault 时展示文件树与编辑器', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({
      id: 1,
      displayName: '库',
      absPath: 'C:/v',
      status: 'ready',
    } as never);
    const wrapper = mountLayout();
    await flushPromises();
    expect(wrapper.find('[data-testid="tree-stub"]').exists()).toBe(true);
  });

  it('文件树打开笔记 → 编辑器打开（FR-FILE-10 的进入编辑态）', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ id: 1, displayName: '库', absPath: 'C:/v' } as never);
    const { noteRead } = await import('@core/ipc/commands');
    vi.mocked(noteRead).mockResolvedValue({ relPath: 'a.md', content: 'x', mtimeMs: 1, sizeBytes: 1 });
    const wrapper = mountLayout();
    await flushPromises();
    wrapper.findComponent(FileTreeStub).vm.$emit('open', 'a.md');
    await flushPromises();
    expect(vi.mocked(noteRead)).toHaveBeenCalledWith('a.md');
  });

  it('右键动作覆盖新建笔记/文件夹/删除（含文件→取其所在目录）', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ id: 1, displayName: '库', absPath: 'C:/v' } as never);
    const wrapper = mountLayout();
    await flushPromises();
    const tree = wrapper.findComponent(FileTreeStub);
    for (const [type, relPath, isDir] of [
      ['newNote', 'dir', true],
      ['newFolder', 'dir', true],
      ['delete', 'dir/a.md', false],
    ] as const) {
      wrapper.find('[data-testid="fileops-cancel"]').exists();
      tree.vm.$emit('action', { type, relPath, isDir });
      await flushPromises();
      expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
      const cancel = wrapper.find('[data-testid="fileops-cancel"]');
      if (cancel.exists()) await cancel.trigger('click');
    }
  });
  it('文件树右键动作 → 打开对应文件操作对话框', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ id: 1, displayName: '库', absPath: 'C:/v' } as never);
    const wrapper = mountLayout();
    await flushPromises();
    wrapper.findComponent(FileTreeStub).vm.$emit('action', { type: 'rename', relPath: 'dir/a.md', isDir: false });
    await flushPromises();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="fileops-name"]').element as HTMLInputElement).toBeTruthy();
  });
});
