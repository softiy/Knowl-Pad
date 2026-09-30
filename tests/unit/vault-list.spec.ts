import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({ vaultList: vi.fn() }));

import { vaultList } from '@core/ipc/commands';
import { useVaultStore, VaultList } from '@features/vault';

const mocked = vi.mocked(vaultList);

describe('VaultList 组件', () => {
  beforeEach(() => mocked.mockReset());

  it('渲染 Vault 列表', async () => {
    mocked.mockResolvedValue([{ id: 1, absPath: 'C:/v1', displayName: '库一' }]);
    const wrapper = mount(VaultList, { global: { plugins: [createPinia()] } });
    await flushPromises();
    expect(wrapper.text()).toContain('库一');
  });

  it('store 有错误时展示错误文案', async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    mocked.mockResolvedValue([]);
    const store = useVaultStore();
    const wrapper = mount(VaultList, { global: { plugins: [pinia] } });
    await flushPromises();
    store.error = '当前没有打开的知识库';
    await flushPromises();
    expect(wrapper.find('.error').text()).toContain('当前没有打开的知识库');
  });

  it('导出 store 可用', () => {
    const store = useVaultStore();
    expect(store.items).toEqual([]);
  });
});
