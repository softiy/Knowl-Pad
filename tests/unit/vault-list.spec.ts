import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const push = vi.fn();
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }));
vi.mock('@core/ipc/commands', () => ({
  vaultList: vi.fn(),
  vaultCurrent: vi.fn(async () => null),
  vaultOpen: vi.fn(),
  vaultCreate: vi.fn(),
  vaultPin: vi.fn(),
  vaultRename: vi.fn(),
  vaultRegisterRemove: vi.fn(),
  vaultRelocate: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  vaultCreate,
  vaultCurrent,
  vaultList,
  vaultOpen,
  vaultPin,
  vaultRegisterRemove,
  vaultRelocate,
} from '@core/ipc/commands';
import { useVaultStore, VaultList } from '@features/vault';

const mList = vi.mocked(vaultList);
const mOpen = vi.mocked(vaultOpen);
const mCreate = vi.mocked(vaultCreate);
const mPin = vi.mocked(vaultPin);
const mRemove = vi.mocked(vaultRegisterRemove);
const mRelocate = vi.mocked(vaultRelocate);
const mDialog = vi.mocked(openDialog);
const VAULT = { root: 'C:/v1', displayName: '库一', vaultId: 1, caseInsensitiveFs: true };
const ITEM = { id: 1, absPath: 'C:/v1', displayName: '库一', pinned: false, lastOpened: 1700000000000 };

function mountList() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const wrapper = mount(VaultList, { global: { plugins: [pinia] } });
  return { wrapper, store: useVaultStore() };
}

describe('VaultList / 欢迎页（FR-VAULT-01/02/03/07/08、AC-VAULT-02）', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mList.mockResolvedValue([ITEM]);
    vi.mocked(vaultCurrent).mockResolvedValue(null);
    mOpen.mockResolvedValue(VAULT);
    mCreate.mockResolvedValue(VAULT);
    mDialog.mockResolvedValue(null);
  });

  it('渲染列表项：名称、路径、最近打开时间与置顶状态', async () => {
    const { wrapper } = mountList();
    await flushPromises();
    expect(wrapper.find('[data-testid="vault-list"]').text()).toContain('库一');
    expect(wrapper.text()).toContain('C:/v1');
    expect(wrapper.find('[data-testid="vault-pin-1"]').text()).toContain('置顶');
  });

  it('「打开已有文件夹」走原生选择框 → vault_open → 进入工作区', async () => {
    mDialog.mockResolvedValue('C:/v1');
    const { wrapper } = mountList();
    await flushPromises();
    await wrapper.find('[data-testid="vault-open"]').trigger('click');
    await flushPromises();
    expect(mDialog).toHaveBeenCalledWith(expect.objectContaining({ directory: true }));
    expect(mOpen).toHaveBeenCalledWith('C:/v1');
    expect(push).toHaveBeenCalledWith('/');
  });

  it('「新建知识库」→ vault_create', async () => {
    mDialog.mockResolvedValue('C:/new');
    const { wrapper } = mountList();
    await flushPromises();
    await wrapper.find('[data-testid="vault-create"]').trigger('click');
    await flushPromises();
    expect(mCreate).toHaveBeenCalledWith('C:/new');
  });

  it('置顶 / 重命名 / 移除（移除只删注册记录）', async () => {
    const { wrapper } = mountList();
    await flushPromises();
    await wrapper.find('[data-testid="vault-pin-1"]').trigger('click');
    expect(mPin).toHaveBeenCalledWith(1, true);

    await wrapper.find('[data-testid="vault-rename-1"]').trigger('click');
    await wrapper.find('[data-testid="vault-rename-input"]').setValue('新名字');
    await wrapper.find('[data-testid="vault-rename-confirm"]').trigger('click');
    await flushPromises();
    expect(vi.mocked((await import('@core/ipc/commands')).vaultRename)).toHaveBeenCalledWith(1, '新名字');

    await wrapper.find('[data-testid="vault-remove-1"]').trigger('click');
    expect(wrapper.find('[data-testid="vault-remove-confirm"]').text()).toContain('不会');
    await wrapper.find('[data-testid="vault-remove-confirm-ok"]').trigger('click');
    await flushPromises();
    expect(mRemove).toHaveBeenCalledWith(1);
  });

  it('AC-VAULT-02：路径失效 → 明确提示 + 重新定位 / 从列表移除', async () => {
    const { wrapper } = mountList();
    await flushPromises();
    mOpen.mockRejectedValueOnce({ code: 'E_VAULT_PATH_INVALID', message: '无法访问知识库「库一」，路径 C:/v1 不存在或不可访问' });
    mRelocate.mockResolvedValue(VAULT);
    mDialog.mockResolvedValue('C:/moved');
    await wrapper.find('[data-testid="vault-item-1"] button').trigger('click');
    await flushPromises();
    const banner = wrapper.find('[data-testid="vault-path-error"]');
    expect(banner.exists()).toBe(true);
    expect(banner.text()).toContain('无法访问知识库');

    await wrapper.find('[data-testid="vault-relocate"]').trigger('click');
    await flushPromises();
    expect(mRelocate).toHaveBeenCalledWith(1, 'C:/moved');
    expect(wrapper.find('[data-testid="vault-path-error"]').exists()).toBe(false);
  });

  it('空列表给出引导文案', async () => {
    mList.mockResolvedValue([]);
    const { wrapper } = mountList();
    await flushPromises();
    expect(wrapper.find('[data-testid="vault-empty"]').exists()).toBe(true);
  });
});
