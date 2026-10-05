import { mount, flushPromises } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  indexStatus: vi.fn(),
  fileTree: vi.fn(async () => []),
  preferenceGet: vi.fn(async () => ({ values: {} })),
  preferenceSet: vi.fn(),
}));

import { fileTree, indexStatus, preferenceGet, preferenceSet } from '@core/ipc/commands';
import StatusBar from '@/app/layout/StatusBar.vue';

const mIndex = vi.mocked(indexStatus);
const mTree = vi.mocked(fileTree);
const mPrefSet = vi.mocked(preferenceSet);

const VAULT = { root: 'C:/vaults/notes', displayName: '我的知识库', vaultId: 1, caseInsensitiveFs: true };

function mountBar(vault: typeof VAULT | null = VAULT) {
  return mount(StatusBar, { props: { vault } });
}

describe('StatusBar（FR-VAULT-11/12、FR-STORAGE-03）', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mTree.mockResolvedValue([]);
    vi.mocked(preferenceGet).mockResolvedValue({ values: {} });
  });

  it('显示 Vault 名称与路径', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    const wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="status-vault"]').text()).toBe('我的知识库');
    expect(wrapper.find('[data-testid="status-path"]').text()).toBe('C:/vaults/notes');
  });

  it('索引三态：就绪 / 索引中 / 索引失败', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    let wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="status-index"]').text()).toContain('就绪');

    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: false });
    wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="status-index"]').text()).toContain('索引中');

    mIndex.mockRejectedValue({ code: 'E_DB_ERROR', message: '索引库损坏' });
    wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="status-index"]').text()).toContain('索引失败');
  });

  it('未打开 Vault 时显示占位且不检测 Git', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    const wrapper = mountBar(null);
    await flushPromises();
    expect(wrapper.find('[data-testid="status-vault"]').text()).toBe('未打开知识库');
    expect(mTree).not.toHaveBeenCalled();
  });

  it('检测到 Git 仓库时提示忽略规则，且**不写任何文件**（只读检测）', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const wrapper = mountBar();
    await flushPromises();
    const hint = wrapper.find('[data-testid="git-hint"]');
    expect(hint.exists()).toBe(true);
    expect(hint.text()).toContain('不会自动修改');
    // 红线：只读检测 + 只读偏好；此时不得有任何写入
    expect(mPrefSet).not.toHaveBeenCalled();
  });

  it('非 Git 仓库不提示', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note' }]);
    const wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="git-hint"]').exists()).toBe(false);
  });

  it('点「复制忽略规则」把 .knowlpad/ 写入剪贴板', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const wrapper = mountBar();
    await flushPromises();
    await wrapper.find('[data-testid="git-hint-copy"]').trigger('click');
    expect(writeText).toHaveBeenCalledWith('.knowlpad/');
  });

  it('「知道了」把提示状态写入偏好（下次不再打扰）', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const wrapper = mountBar();
    await flushPromises();
    await wrapper.find('[data-testid="git-hint-dismiss"]').trigger('click');
    expect(mPrefSet).toHaveBeenCalledWith({ 'ui.gitHintDismissed': true });
    expect(wrapper.find('[data-testid="git-hint"]').exists()).toBe(false);
  });

  it('已关闭过提示则不再显示', async () => {
    vi.mocked(preferenceGet).mockResolvedValue({ values: { 'ui.gitHintDismissed': true } });
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="git-hint"]').exists()).toBe(false);
  });
});
