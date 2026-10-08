import { mount, flushPromises } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@features/vault', async () => {
  // 真实项目用 Pinia store（ref 会被自动解包），这里用 reactive 复现同样的语义：
  // 组件模板写的是 `vaultStore.current.displayName`，裸 ref 会得到 undefined。
  const { ref, reactive } = await import('vue');
  const VAULT = { root: 'C:/vaults/notes', displayName: '我的知识库', vaultId: 1, caseInsensitiveFs: true };
  const current = ref<typeof VAULT | null>({ ...VAULT });
  const error = ref<string | null>(null);
  const store = reactive({
    current,
    error,
    init: vi.fn(async () => {}),
    closeCurrent: vi.fn(async () => {
      current.value = null;
    }),
  });
  return { useVaultStore: () => store };
});

const saveAll = vi.fn(async () => {});
const dirtyFlag = { value: false };
const hasUnsaved = vi.fn(() => false);
const buffers = [{ relPath: 'a.md' }];
vi.mock('@features/editor', async () => {
  return {
    EditorView: { template: '<div data-testid="editor" />' },
    useEditorStore: () => ({
      buffers,
      saveAll,
      hasUnsaved,
      openNote: vi.fn(async () => {}),
      isDirty: vi.fn(() => dirtyFlag.value),
    }),
  };
});

vi.mock('@features/file-tree', () => ({
  FileTree: { template: '<div />' },
  FileOpsDialog: { template: '<div />' },
  useFileOpsStore: () => ({ notice: null, openNewNote: vi.fn(), openNewFolder: vi.fn(), openRename: vi.fn(), openDelete: vi.fn() }),
}));
vi.mock('@features/vault/components/VaultList.vue', () => ({ default: { template: '<div data-testid="vault-list" />' } }));
vi.mock('@core/ipc/commands', () => ({ indexStatus: vi.fn(async () => ({ ready: true })), fileTree: vi.fn(async () => []), preferenceGet: vi.fn(async () => ({ values: {} })), preferenceSet: vi.fn() }));
vi.mock('@core/ipc/events', () => ({ onIndexProgress: vi.fn(async () => () => {}), onIndexCompleted: vi.fn(async () => () => {}), onIndexFailed: vi.fn(async () => () => {}) }));

import WorkspaceLayout from '@/app/layout/WorkspaceLayout.vue';

describe('工作区：切换知识库入口（FR-VAULT-04/05、AC-VAULT-03 的落盘）', () => {
  beforeEach(async () => {
    saveAll.mockClear();
    hasUnsaved.mockReset();
    hasUnsaved.mockReturnValue(false);
    // mock 是**共享**的 reactive store：上一个用例若切换过，current 会是 null，
    // 于是下一个用例连按钮都渲染不出来。每个用例都恢复成"已打开 Vault"。
    const { useVaultStore } = await import('@features/vault');
    useVaultStore().current = { ...VAULT };
  });

  it('打开 Vault 时提供切换入口，并把 Vault 名显示出来', async () => {
    const w = mount(WorkspaceLayout);
    await flushPromises();
    expect(w.find('[data-testid="workspace-switch-vault"]').exists()).toBe(true);
    expect(w.find('[data-testid="workspace-vault-name"]').text()).toContain('我的知识库');
  });

  it('点击切换：先刷盘再关闭，随后回到欢迎页列表', async () => {
    const w = mount(WorkspaceLayout);
    await flushPromises();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(saveAll).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="vault-list"]').exists()).toBe(true);
  });

  it('仍有未保存内容时不切换，并给出可见提示（R-07：不静默丢内容）', async () => {
    hasUnsaved.mockReturnValue(true);
    dirtyFlag.value = true;
    const w = mount(WorkspaceLayout);
    await flushPromises();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(w.find('[data-testid="workspace-switch-error"]').text()).toContain('已取消切换');
    expect(w.find('[data-testid="vault-list"]').exists()).toBe(false);
    dirtyFlag.value = false;
  });
});
