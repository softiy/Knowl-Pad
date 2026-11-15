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
import { onIndexCancelled, onIndexCompleted, onIndexFailed, onIndexProgress } from '@core/ipc/events';
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
/** 本次手动重建返回的警告（显式动作的结果必须可见，不能只留在返回值里）。 */
const rebuildWarnings = ref<string[]>([]);
let unlisten: UnlistenFn[] = [];

/**
 * FR-VAULT-11 的四种状态：**未打开** / 就绪 / 索引中 / 索引失败。
 *
 * 「未打开」是 2026-10-21 补的（**DEBT-44**）：此前没有 Vault 时走 `ready:false` 分支显示"索引中"，
 * 而实际上什么都没在索引 —— 属**用户可见的假状态**（与"不要把现状说成别的"这条纪律冲突）。
 */
const label = computed(() => {
  if (!props.vault) return '未打开';
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

/** 悬停明细最多列出的条数：状态栏放不下全量，完整清单仍在 `index.warnings` 里。 */
const MAX_WARNING_LINES = 5;

/**
 * **最近一次索引的非致命警告**（PRD §5.3.7 / 勘误 D-46）。
 *
 * T1 的根因：启动对账把 `outcome.warnings` 丢了，而 `kp://index/completed` 的字段集合
 * 由 PRD §5.4 定死（EVT-03，不得扩展）—— 警告只能作为**可轮询的状态**经 `index_status` 出来。
 * 因此这里不做一次性提示，而是每次 refresh（含 completed 事件后）都从状态里读。
 */
const warnings = computed(() => index.value?.warnings ?? []);

/** 明细：前 N 条（含 `skip-timeout:<文件名>` 这类可诊断路径），其余只报剩余条数。 */
function formatWarnings(list: string[]): string {
  const shown = list.slice(0, MAX_WARNING_LINES).join('\n');
  return list.length > MAX_WARNING_LINES ? `${shown}\n…另有 ${list.length - MAX_WARNING_LINES} 条` : shown;
}

const warningsTitle = computed(() =>
  `最近一次索引有 ${warnings.value.length} 条警告（超时/超限的文件已跳过、未进入索引）：\n${formatWarnings(warnings.value)}`,
);
const rebuildWarningsTitle = computed(
  () => `本次重建跳过/告警的文件：\n${formatWarnings(rebuildWarnings.value)}`,
);

/**
 * 拉一次索引状态 + 规模详情。
 *
 * **没有 Vault 时直接返回**（**DEBT-44**）：`index_stats` 需要打开的 Vault（`index_stats_impl` 的守卫
 * 返回 `E_VAULT_NOT_OPEN`），而本组件挂在 `StatusBar` 里、**不随 Vault 卸载**
 * （`WorkspaceLayout.vue` 的 `v-if` 只包住工作区），所以关闭/切换 Vault 时 watch 仍会触发一次 ——
 * 以前那条命令必然失败，由 `error_wrapper` 记一条 WARN（**每次切换一条假失败日志**），
 * 再被下面的 catch 静默吞掉。现在在调用点就收手。
 * 注意：`indexStatus` 本身不需要 Vault（无索引库时返回 `ready:false`），但仍一并不发 ——
 * 没有 Vault 时它的返回值对界面毫无意义，留着只是多一次 IPC。
 */
async function refresh(): Promise<void> {
  if (!props.vault) {
    index.value = null;
    stats.value = null;
    failed.value = false;
    error.value = null;
    rebuildWarnings.value = [];
    return;
  }
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
  rebuildWarnings.value = [];
  try {
    // T1 的另一半：手工重建此前只把 warnings 放在返回值里、界面无人展示。
    // 这里把它接住；后端同时会写进 AppState，finally 的 refresh 还会经 index_status 再取一次。
    const result = await indexRebuild(true);
    // 取消 ⇒ 索引不可信（与后端 clear 一致），不展示半程警告；完成时把跳过的文件如实列出来。
    rebuildWarnings.value = result.cancelled ? [] : result.warnings;
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
      // DEBT-45：取消 ≠ 完成 —— 隐藏进度并刷新（refresh 会读到 ready=false 并显示未就绪）
      await onIndexCancelled(() => {
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
    <span
      v-if="warnings.length"
      class="kp-index__warn"
      data-testid="index-warnings"
      :title="warningsTitle"
    >
      ⚠ {{ warnings.length }} 条索引警告
    </span>
    <span
      v-if="rebuildWarnings.length"
      class="kp-index__warn"
      data-testid="index-rebuild-warnings"
      :title="rebuildWarningsTitle"
    >
      重建完成：{{ rebuildWarnings.length }} 条警告（部分文件被跳过）
    </span>
    <span v-if="actionError" class="kp-index__error" data-testid="index-rebuild-error">
      {{ actionError }}
    </span>
  </span>
</template>

<style scoped>
.kp-index { margin-left: auto; display: inline-flex; align-items: center; gap: 8px; }
.kp-index__error { color: #b00020; }
/* 与状态栏 Git 提示/搜索"范围过大"同一套橙色（StatusBar.vue 的 .kp-status__hint 用的就是它）。 */
.kp-index__warn { color: #b26a00; cursor: help; }
</style>
