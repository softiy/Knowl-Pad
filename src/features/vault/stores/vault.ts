import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import { vaultList, type VaultSummary } from '@core/ipc/commands';
import type { VaultListItem } from '../types';

export const useVaultStore = defineStore('vault', () => {
  const items = ref<VaultListItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  function mapOne(v: VaultSummary): VaultListItem {
    return { id: v.id, displayName: v.displayName, absPath: v.absPath };
  }

  async function refresh(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      items.value = (await vaultList()).map(mapOne);
    } catch (err) {
      error.value = asKpError(err).message;
    } finally {
      loading.value = false;
    }
  }

  return { items, loading, error, refresh };
});
