<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useVaultStore } from '../stores/vault';
import type { VaultListItem } from '../types';

/**
 * 欢迎页 / Vault 选择（PRD §7.1 的 V-WELCOME；FR-VAULT-01/02/03/07/08）。
 *
 * 这是**唯一**的 Vault 入口：打开已有文件夹、新建知识库、切换、置顶、重命名显示名、从列表移除；
 * 路径失效时按 AC-VAULT-02 给出提示与「重新定位 / 从列表移除」两个操作。
 * `移除`只删注册记录，**绝不**删除磁盘文件（FR-VAULT-07）。
 */
const store = useVaultStore();
const router = useRouter();
const renaming = ref<{ id: number; value: string } | null>(null);
const pendingRemove = ref<VaultListItem | null>(null);

onMounted(() => {
  void store.init();
});

function formatTime(ms?: number): string {
  if (!ms) return '';
  return new Date(ms).toLocaleString();
}

async function enter(): Promise<void> {
  await router.push('/');
}

async function openExisting(): Promise<void> {
  if (await store.open()) await enter();
}

async function createNew(): Promise<void> {
  if (await store.create()) await enter();
}

async function activate(item: VaultListItem): Promise<void> {
  if (await store.open(item.absPath)) await enter();
}

function startRename(item: VaultListItem): void {
  renaming.value = { id: item.id, value: item.displayName };
}

async function confirmRename(): Promise<void> {
  const target = renaming.value;
  renaming.value = null;
  if (!target || !target.value.trim()) return;
  await store.rename(target.id, target.value.trim());
}

async function confirmRemove(): Promise<void> {
  const target = pendingRemove.value;
  pendingRemove.value = null;
  if (target) await store.remove(target.id);
}
</script>

<template>
  <section class="kp-welcome" data-testid="vault-welcome">
    <h1 class="kp-welcome__title">Knowl Pad</h1>
    <p class="kp-welcome__subtitle">本地优先的 Markdown 知识库</p>

    <div class="kp-welcome__actions">
      <button type="button" data-testid="vault-open" @click="openExisting">打开已有文件夹</button>
      <button type="button" data-testid="vault-create" @click="createNew">新建知识库</button>
    </div>

    <p v-if="store.loading" data-testid="vault-loading">加载中…</p>
    <p v-if="store.error" class="kp-welcome__error" data-testid="vault-error">{{ store.error }}</p>

    <!-- AC-VAULT-02：路径失效 → 明确提示 + 重新定位 / 从列表移除，绝不崩溃、不静默建目录 -->
    <div v-if="store.pathError" class="kp-welcome__path-error" data-testid="vault-path-error">
      <p>{{ store.pathError.message }}</p>
      <button type="button" data-testid="vault-relocate" @click="store.relocate(store.pathError.vault.id)">
        重新定位
      </button>
      <button type="button" data-testid="vault-remove" @click="pendingRemove = store.pathError.vault">
        从列表移除
      </button>
    </div>

    <ul class="kp-welcome__list" data-testid="vault-list">
      <li v-for="item in store.items" :key="item.id" :data-testid="`vault-item-${item.id}`">
        <button type="button" class="kp-welcome__open" @click="activate(item)">
          <span v-if="item.pinned" aria-hidden="true">📌</span>
          <span class="kp-welcome__name">{{ item.displayName }}</span>
          <span class="kp-welcome__path">{{ item.absPath }}</span>
          <span v-if="item.lastOpened" class="kp-welcome__time">{{ formatTime(item.lastOpened) }}</span>
        </button>
        <button type="button" :data-testid="`vault-pin-${item.id}`" @click="store.pin(item.id, !item.pinned)">
          {{ item.pinned ? '取消置顶' : '置顶' }}
        </button>
        <button type="button" :data-testid="`vault-rename-${item.id}`" @click="startRename(item)">重命名</button>
        <button type="button" :data-testid="`vault-remove-${item.id}`" @click="pendingRemove = item">移除</button>
      </li>
      <li v-if="!store.loading && store.items.length === 0" data-testid="vault-empty">
        还没有知识库：可以「打开已有文件夹」，或「新建知识库」
      </li>
    </ul>

    <div v-if="renaming" class="kp-welcome__inline" data-testid="vault-rename-form">
      <input v-model="renaming.value" data-testid="vault-rename-input" @keyup.enter="confirmRename">
      <button type="button" data-testid="vault-rename-confirm" @click="confirmRename">确定</button>
      <button type="button" @click="renaming = null">取消</button>
    </div>

    <div v-if="pendingRemove" class="kp-welcome__inline" data-testid="vault-remove-confirm">
      <span>从列表移除「{{ pendingRemove.displayName }}」？（仅删除注册记录，**不会**删除磁盘文件）</span>
      <button type="button" data-testid="vault-remove-confirm-ok" @click="confirmRemove">移除</button>
      <button type="button" @click="pendingRemove = null">取消</button>
    </div>
  </section>
</template>

<style scoped>
.kp-welcome { max-width: 640px; margin: 0 auto; padding: 32px 16px; font-size: 14px; }
.kp-welcome__title { margin: 0; font-size: 24px; }
.kp-welcome__subtitle { margin: 4px 0 20px; color: #666; }
.kp-welcome__actions { display: flex; gap: 8px; margin-bottom: 16px; }
.kp-welcome__error { color: #b00020; }
.kp-welcome__path-error { margin: 8px 0; padding: 8px; background: #fff3f3; border: 1px solid #f3c2c2; border-radius: 4px; }
.kp-welcome__list { list-style: none; margin: 0; padding: 0; }
.kp-welcome__list li { display: flex; align-items: center; gap: 6px; padding: 6px 0; border-bottom: 1px solid #eee; }
.kp-welcome__open { flex: 1; display: flex; gap: 8px; align-items: baseline; background: none; border: 0; cursor: pointer; text-align: left; }
.kp-welcome__name { font-weight: 600; }
.kp-welcome__path { color: #888; font-size: 12px; }
.kp-welcome__time { margin-left: auto; color: #aaa; font-size: 12px; }
.kp-welcome__inline { display: flex; gap: 8px; align-items: center; margin-top: 12px; padding: 8px; background: #f7f7f7; border-radius: 4px; }
</style>
