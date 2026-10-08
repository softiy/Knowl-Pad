<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import {
  indexCancel,
  indexRebuild,
  indexStats,
  indexStatus,
  type IndexStats,
  type IndexStatus,
  type VaultInfo,
} from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import { onIndexCompleted, onIndexFailed, onIndexProgress } from '@core/ipc/events';
import type { UnlistenFn } from '@tauri-apps/api/event';

/**
 * 索引状态徽标（FR-VAULT-11 状态显示、FR-VAULT-09 可取消）。
 *
 * 从 StatusBar 拆出：状态栏自己还要承担 Git 忽略提示（FR-VAULT-12），
 * 两者放一个文件会越过 CODE-11 的 200 行警告线。
 *
 * 这里给此前**前端零消费**的三个命令接上真实入口：索引统计（详情）、重建（补救）、取消。
 */
const props = defineProps<{ vault: VaultInfo | null }>();

const index = ref<IndexStatus | null>(null);
const stats = ref<IndexStats | null>(null);
const failed = ref(false);
const error = ref<string | null>(null);
const progress = ref<{ done: number; total: number } | null>(null);
const rebuilding = ref(false);
const actionError = ref<string | null>(null);
let unlisten: UnlistenFn[] = [];

/** FR-VAULT-11 的三种状态：就绪 / 索引中 / 索引失败。 */
const label = computed(() => {
  if (failed.value) return '索引失败';
  if (progress.value) {
    const p = progress.value;
    return p.total > 0 ? '索引中 ' + p.done + '/' + p.total : '索引中';
  }
  if (!index.value) return '索引中';
  return index.value.ready ? '就绪' : '索引中';
});

/** 详情（悬停可见）：就绪应当能说明规模。 */
const detail = computed(() => {
  const s = stats.value;
  if (!s) return '';
  return '文件 ' + s.files + ' · 链接 ' + s.links + ' · 标签 ' + s.tags;
});

async function refresh(): Promise<void> {
  failed.value = false;
  try {
    index.value = await indexStatus();
  } catch (err) {
    failed.value = true;
    error.value = asKpError(err).message;
    return;
  }
  try {
    stats.value = await indexStats();
  } catch {
    stats.value = null; // 统计只是详情，拿不到不影响主状态
  }
}

async function rebuild(): Promise<void> {
  rebuilding.value = true;
  actionError.value = null;
  try {
    await indexRebuild(true);
  } catch (err) {
    actionError.value = asKpError(err).message;
  } finally {
    rebuilding.value = false;
    await refresh();
  }
}

async function cancel(): Promise<void> {
  try {
    await indexCancel();
  } catch (err) {
    actionError.value = asKpError(err).message;
  }
}

onMounted(async () => {
  await refresh();
  try {
    unlisten = [
      await onIndexProgress((p) => {
        progress.value = { done: p.done, total: p.total };
      }),
      await onIndexCompleted(() => {
        progress.value = null;
        void refresh();
      }),
      await onIndexFailed((e) => {
        progress.value = null;
        failed.value = true;
        error.value = e.message;
      }),
    ];
  } catch (err) {
    // R-15：不空吞 —— 事件通道不可用时给出可见错误
    error.value = asKpError(err).message;
  }
});
onUnmounted(() => {
  for (const off of unlisten) off();
  unlisten = [];
});
/** M9：Vault 切换/关闭后必须重算，否则停留在上一个 Vault 的状态上。 */
watch(
  () => props.vault?.root,
  () => void refresh(),
);
</script>

<template>
  <span class="kp-index" data-testid="status-index" :title="detail">
    索引：{{ label }}
    <button v-if="!rebuilding" type="button" data-testid="index-rebuild" @click="rebuild">
      重建索引
    </button>
    <button v-else type="button" data-testid="index-cancel" @click="cancel">取消</button>
    <span v-if="actionError" class="kp-index__error" data-testid="index-rebuild-error">
      {{ actionError }}
    </span>
  </span>
</template>

<style scoped>
.kp-index { margin-left: auto; display: inline-flex; align-items: center; gap: 8px; }
.kp-index__error { color: #b00020; }
</style>
