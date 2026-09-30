import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

vi.mock('@core/ipc/commands', () => ({
  vaultList: vi.fn(),
}));

import { vaultList } from '@core/ipc/commands';
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
