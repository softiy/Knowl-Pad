<script setup lang="ts">
import { onMounted } from 'vue';
import { useVaultStore } from '../stores/vault';

const store = useVaultStore();
onMounted(() => {
  void store.refresh();
});
</script>

<template>
  <ul class="vault-list">
    <li v-if="store.loading">加载中…</li>
    <li v-else-if="store.error" class="error">{{ store.error }}</li>
    <li v-for="v in store.items" :key="v.id">{{ v.displayName }} — {{ v.absPath }}</li>
  </ul>
</template>
