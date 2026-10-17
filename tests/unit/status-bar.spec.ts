import { mount, flushPromises } from '@vue/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  indexStatus: vi.fn(),
  fileTree: vi.fn(async () => []),
  preferenceGet: vi.fn(async () => ({ values: {} })),
  preferenceSet: vi.fn(),
}));

vi.mock('@core/ipc/events', () => ({
  onIndexProgress: vi.fn(async () => () => {}),
  onIndexCompleted: vi.fn(async () => () => {}),
  onIndexFailed: vi.fn(async () => () => {}),
}));

import { fileTree, indexStatus, preferenceGet, preferenceSet } from '@core/ipc/commands';
import { flushPreferences, pendingPreferenceCount } from '@core/ipc/preferences';
import StatusBar from '@/app/layout/StatusBar.vue';

// 偏好写入走 500ms 防抖队列（FR-SET-03 / PERF-06）：每个用例结束前清空，避免定时器跨用例触发。
afterEach(async () => {
  await flushPreferences();
});

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
    // 红线：只读检测 + 只读偏好；此时不得有任何写入（含防抖队列里也不能有待写项）
    expect(mPrefSet).not.toHaveBeenCalled();
    expect(pendingPreferenceCount()).toBe(0);
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
    // 写入先进防抖队列（500ms 后才提交），由 flush 立刻兑现 —— 语义仍是"点了就记住"
    expect(pendingPreferenceCount()).toBe(1);
    expect(mPrefSet).not.toHaveBeenCalled();
    await flushPreferences();
    expect(mPrefSet).toHaveBeenCalledWith({ 'ui.gitHintDismissed': true });
    expect(wrapper.find('[data-testid="git-hint"]').exists()).toBe(false);
  });

  it('M9：切换 Vault 后重新计算状态与提示（不是只读一次）', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="git-hint"]').exists()).toBe(true);
    // 切到一个非 Git 仓库：提示必须消失
    mTree.mockResolvedValue([{ relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note' }]);
    await wrapper.setProps({ vault: { ...VAULT, root: 'C:/other', displayName: '另一个库' } });
    await flushPromises();
    expect(wrapper.find('[data-testid="status-vault"]').text()).toBe('另一个库');
    expect(wrapper.find('[data-testid="git-hint"]').exists()).toBe(false);
  });

  it('R-15：Git 检测失败可见，不伪装成「没有 Git 仓库」', async () => {
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockRejectedValueOnce({ code: 'E_IO_FAILURE', message: '目录不可读' });
    const wrapper = mountBar();
    await flushPromises();
    expect(wrapper.find('[data-testid="git-check-failed"]').exists()).toBe(true);
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

describe('复制忽略规则要如实反馈（审计 F4）', () => {
  // 本 describe 必须自己重建 mock：上面那个 beforeEach 里的 vi.resetAllMocks()
  // 会把 mock 工厂的实现也清掉（preferenceGet 变 undefined → 组件内部报错 → 提示条不渲染）。
  beforeEach(() => {
    vi.resetAllMocks();
    mTree.mockResolvedValue([]);
    vi.mocked(preferenceGet).mockResolvedValue({ values: {} });
  });

  it('复制失败不得显示"已复制"，要给可操作提示', async () => {
    // 让 writeText 抛（浏览器拒绝剪贴板是常见情形）
    const writeText = vi.fn().mockRejectedValue(new Error('denied'));
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    // 与既有 git-hint 用例同样的构造：索引就绪 + 文件树里有 .git，提示条才会渲染
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const w = mountBar();
    await flushPromises();
    await w.get('[data-testid="git-hint-copy"]').trigger('click');
    await flushPromises();
    expect(w.get('[data-testid="git-hint-copy"]').text()).toContain('复制忽略规则');
    expect(w.get('[data-testid="git-hint-copy"]').text()).not.toContain('已复制');
    expect(w.get('[data-testid="git-hint-copy-error"]').text()).toContain('手动');
  });

  it('复制成功才显示"已复制"', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    mIndex.mockResolvedValue({ indexDir: 'd', indexDb: 'db', ready: true });
    mTree.mockResolvedValue([{ relPath: '.git', name: '.git', isDir: true, kind: 'other' }]);
    const w = mountBar();
    await flushPromises();
    await w.get('[data-testid="git-hint-copy"]').trigger('click');
    await flushPromises();
    expect(writeText).toHaveBeenCalled();
    expect(w.get('[data-testid="git-hint-copy"]').text()).toContain('已复制');
  });
});
