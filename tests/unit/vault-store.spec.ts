import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

vi.mock('@core/ipc/commands', () => ({
  vaultList: vi.fn(),
  vaultCurrent: vi.fn(),
  vaultOpen: vi.fn(),
  vaultCreate: vi.fn(),
  vaultPin: vi.fn(),
  vaultRename: vi.fn(),
  vaultRegisterRemove: vi.fn(),
  vaultRelocate: vi.fn(),
  vaultClose: vi.fn(),
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
  vaultRename,
} from '@core/ipc/commands';
import { useVaultStore } from '@features/vault/stores/vault';

const mocked = vi.mocked(vaultList);

describe('useVaultStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    mocked.mockReset();
  });

  it('成功刷新列表', async () => {
    mocked.mockResolvedValue([{ id: 1, absPath: 'C:/v', displayName: 'v' }]);
    const store = useVaultStore();
    await store.refresh();
    expect(store.items).toEqual([{ id: 1, displayName: 'v', absPath: 'C:/v' }]);
    expect(store.error).toBeNull();
    expect(store.loading).toBe(false);
  });

  it('失败时记录 KpError 的中文提示且不抛出', async () => {
    mocked.mockRejectedValue({ code: 'E_VAULT_NOT_OPEN', message: '当前没有打开的知识库' });
    const store = useVaultStore();
    await store.refresh();
    expect(store.error).toBe('当前没有打开的知识库');
    expect(store.items).toEqual([]);
  });
});

describe('useVaultStore · Vault 入口流程（FR-VAULT-01/02/03/07/08）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    mocked.mockResolvedValue([]);
  });

  it('open：选择框取消时不做任何事', async () => {
    vi.mocked(openDialog).mockResolvedValue(null);
    const store = useVaultStore();
    expect(await store.open()).toBe(false);
    expect(vaultOpen).not.toHaveBeenCalled();
  });

  it('open：成功后 current 更新并刷新列表', async () => {
    vi.mocked(openDialog).mockResolvedValue('C:/v');
    vi.mocked(vaultOpen).mockResolvedValue({ root: 'C:/v', displayName: 'v', vaultId: 1, caseInsensitiveFs: true });
    const store = useVaultStore();
    expect(await store.open()).toBe(true);
    expect(store.current?.root).toBe('C:/v');
    expect(mocked).toHaveBeenCalled();
  });

  it('open：路径失效且已在列表中 → 进入 AC-VAULT-02 的引导态', async () => {
    mocked.mockResolvedValue([{ id: 7, absPath: 'C:/gone', displayName: '库', pinned: false }]);
    await useVaultStore().refresh();
    vi.mocked(vaultOpen).mockRejectedValue({ code: 'E_VAULT_PATH_INVALID', message: '无法访问知识库「库」' });
    const store = useVaultStore();
    expect(await store.open('C:/gone')).toBe(false);
    expect(store.pathError?.vault.id).toBe(7);
    expect(store.pathError?.message).toContain('无法访问');
  });

  it('open：其它错误只记 error，不进入路径引导态', async () => {
    vi.mocked(vaultOpen).mockRejectedValue({ code: 'E_IO_FAILURE', message: '读写失败' });
    const store = useVaultStore();
    expect(await store.open('C:/v')).toBe(false);
    expect(store.pathError).toBeNull();
    expect(store.error).toBe('读写失败');
  });

  it('create：失败时记录错误', async () => {
    vi.mocked(vaultCreate).mockRejectedValue({ code: 'E_IO_FAILURE', message: '无法创建知识库' });
    const store = useVaultStore();
    expect(await store.create('C:/new')).toBe(false);
    expect(store.error).toBe('无法创建知识库');
  });

  it('remove：移除后清掉指向它的路径引导态', async () => {
    mocked.mockResolvedValue([{ id: 7, absPath: 'C:/gone', displayName: '库', pinned: false }]);
    const store = useVaultStore();
    await store.refresh();
    vi.mocked(vaultOpen).mockRejectedValue({ code: 'E_VAULT_PATH_INVALID', message: '无法访问知识库「库」' });
    await store.open('C:/gone');
    expect(store.pathError).not.toBeNull();
    await store.remove(7);
    expect(vaultRegisterRemove).toHaveBeenCalledWith(7);
    expect(store.pathError).toBeNull();
  });

  it('relocate：重新定位成功后清除引导态', async () => {
    vi.mocked(openDialog).mockResolvedValue('C:/moved');
    vi.mocked(vaultRelocate).mockResolvedValue({ root: 'C:/moved', displayName: '库', vaultId: 7, caseInsensitiveFs: true });
    const store = useVaultStore();
    await store.relocate(7);
    expect(vaultRelocate).toHaveBeenCalledWith(7, 'C:/moved');
    expect(store.pathError).toBeNull();
  });

  it('pin / rename 转发到命令', async () => {
    const store = useVaultStore();
    await store.pin(3, true);
    await store.rename(3, '新名');
    expect(vaultPin).toHaveBeenCalledWith(3, true);
    expect(vaultRename).toHaveBeenCalledWith(3, '新名');
  });

  it('restoreCurrent：命令失败时记错误不抛出', async () => {
    vi.mocked(vaultCurrent).mockRejectedValue({ code: 'E_DB_ERROR', message: '全局库不可用' });
    const store = useVaultStore();
    await store.restoreCurrent();
    expect(store.error).toBe('全局库不可用');
  });
});
