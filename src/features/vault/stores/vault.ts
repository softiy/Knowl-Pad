import { defineStore } from 'pinia';
import { ref } from 'vue';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { asKpError } from '@core/ipc/errors';
import {
  vaultClose,
  vaultCreate,
  vaultCurrent,
  vaultList,
  vaultOpen,
  vaultPin,
  vaultRegisterRemove,
  vaultRelocate,
  vaultRename,
  type VaultInfo,
} from '@core/ipc/commands';
import type { VaultListItem } from '../types';

/**
 * Vault 域状态（FR-VAULT-01/02/03/07/08、AC-VAULT-02）。
 *
 * 这里是「当前 Vault」的**唯一来源**：布局与状态栏都读 `current`，因此切换 Vault 会自然传播
 * （此前布局只在 onMounted 取一次，导致状态栏的 watch 永不触发——M1/M2 复核发现）。
 */
export const useVaultStore = defineStore('vault', () => {
  const items = ref<VaultListItem[]>([]);
  const current = ref<VaultInfo | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** AC-VAULT-02：路径失效时保留「是哪个 Vault」+ 明确提示，供「重新定位 / 从列表移除」使用 */
  const pathError = ref<{ vault: VaultListItem; message: string } | null>(null);

  async function refresh(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      items.value = await vaultList();
    } catch (err) {
      error.value = asKpError(err).message;
    } finally {
      loading.value = false;
    }
  }

  /** 启动时恢复上次打开的 Vault（FR-VAULT-06）。 */
  async function restoreCurrent(): Promise<void> {
    try {
      current.value = await vaultCurrent();
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  async function init(): Promise<void> {
    await Promise.all([refresh(), restoreCurrent()]);
  }

  /** 原生文件夹选择（tauri-plugin-dialog，capability 只放行 dialog:allow-open）。 */
  async function pickDirectory(): Promise<string | null> {
    const picked = await openDialog({ directory: true, multiple: false, title: '选择知识库文件夹' });
    return typeof picked === 'string' ? picked : null;
  }

  /** 打开已有文件夹（FR-VAULT-03）；`absPath` 省略时弹选择框。返回是否成功。 */
  async function open(absPath?: string): Promise<boolean> {
    const target = absPath ?? (await pickDirectory());
    if (!target) return false;
    error.value = null;
    try {
      current.value = await vaultOpen(target);
      pathError.value = null;
      await refresh();
      return true;
    } catch (err) {
      const kp = asKpError(err);
      const known = items.value.find((v) => v.absPath === target);
      if (kp.code === 'E_VAULT_PATH_INVALID' && known) {
        pathError.value = { vault: known, message: kp.message };
      } else {
        error.value = kp.message;
      }
      return false;
    }
  }

  /** 新建知识库（FR-VAULT-02）。 */
  async function create(absPath?: string): Promise<boolean> {
    const target = absPath ?? (await pickDirectory());
    if (!target) return false;
    error.value = null;
    try {
      current.value = await vaultCreate(target);
      await refresh();
      return true;
    } catch (err) {
      error.value = asKpError(err).message;
      return false;
    }
  }

  async function closeCurrent(): Promise<void> {
    try {
      await vaultClose();
      current.value = null;
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  async function pin(vaultId: number, pinned: boolean): Promise<void> {
    try {
      await vaultPin(vaultId, pinned);
      await refresh();
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  async function rename(vaultId: number, displayName: string): Promise<void> {
    try {
      await vaultRename(vaultId, displayName);
      await refresh();
      if (current.value?.vaultId === vaultId) await restoreCurrent();
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /** 从列表移除（FR-VAULT-07）：只删注册记录，绝不删除磁盘文件。 */
  async function remove(vaultId: number): Promise<void> {
    try {
      await vaultRegisterRemove(vaultId);
      if (pathError.value?.vault.id === vaultId) pathError.value = null;
      await refresh();
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /** 重新定位（AC-VAULT-02）：为失效的注册项选择新路径。 */
  async function relocate(vaultId: number): Promise<void> {
    const target = await pickDirectory();
    if (!target) return;
    try {
      current.value = await vaultRelocate(vaultId, target);
      pathError.value = null;
      await refresh();
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  return {
    items,
    current,
    loading,
    error,
    pathError,
    refresh,
    restoreCurrent,
    init,
    pickDirectory,
    open,
    create,
    closeCurrent,
    pin,
    rename,
    remove,
    relocate,
  };
});
