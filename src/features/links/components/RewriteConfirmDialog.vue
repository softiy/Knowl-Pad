<script setup lang="ts">
import { computed, ref } from 'vue';
import {
  linkRewriteApply,
  linkRewritePreview,
  linkRewriteRollback,
  type RewritePreview,
} from '@core/ipc/linkCommands';
import { asKpError } from '@core/ipc/errors';

/**
 * 改写的**两步确认**（FR-FILE-21 + PRD §5.3.3 的 preview_id 流程）。
 *
 * 第一步只读预览："将修改 N 个文件中的 M 处"；用户确认后才执行（一次性消费 previewId）。
 * 执行成功后提供**回滚**入口（AC-FILE-02）。
 *
 * 传入 rename 时，重命名与改写属于**同一次可回滚操作**（FR-FILE-22）：
 * 此时不要再调用 file_rename —— PRD §5.3.3 明确 file_rename「不含链接改写」。
 */
const props = defineProps<{ renameFrom?: string; renameTo?: string }>();

const fromRef = ref('');
const toRef = ref('');
const preview = ref<RewritePreview | null>(null);
const applied = ref<{ operationId: string; fileCount: number; spanCount: number } | null>(null);
const error = ref<string | null>(null);
const busy = ref(false);

const renameSpec = computed(() =>
  props.renameFrom && props.renameTo ? { from: props.renameFrom, to: props.renameTo } : undefined,
);

async function doPreview(): Promise<void> {
  busy.value = true;
  error.value = null;
  applied.value = null;
  try {
    preview.value = await linkRewritePreview(fromRef.value, toRef.value, renameSpec.value);
  } catch (err) {
    error.value = asKpError(err).message;
  } finally {
    busy.value = false;
  }
}

async function doApply(): Promise<void> {
  if (!preview.value) return;
  busy.value = true;
  error.value = null;
  try {
    applied.value = await linkRewriteApply(preview.value.previewId, renameSpec.value);
    preview.value = null;
  } catch (err) {
    error.value = asKpError(err).message;
  } finally {
    busy.value = false;
  }
}

async function doRollback(): Promise<void> {
  if (!applied.value) return;
  busy.value = true;
  error.value = null;
  try {
    await linkRewriteRollback(applied.value.operationId);
    applied.value = null;
  } catch (err) {
    error.value = asKpError(err).message;
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="kp-rewrite" data-testid="rewrite-dialog">
    <label>把 <input v-model="fromRef" data-testid="rewrite-from" placeholder="A"> 改成
      <input v-model="toRef" data-testid="rewrite-to" placeholder="B"></label>
    <button type="button" :disabled="busy || !fromRef || !toRef" data-testid="rewrite-preview" @click="doPreview">
      预览
    </button>

    <p v-if="preview" data-testid="rewrite-preview-summary">
      将修改 {{ preview.fileCount }} 个文件中的 {{ preview.spanCount }} 处链接
    </p>
    <ul v-if="preview" data-testid="rewrite-preview-items">
      <li v-for="e in preview.edits" :key="e.relPath">{{ e.relPath }}（{{ e.hits }} 处）</li>
    </ul>
    <button v-if="preview" type="button" :disabled="busy" data-testid="rewrite-apply" @click="doApply">
      确认执行
    </button>

    <p v-if="applied" data-testid="rewrite-applied">
      已改写 {{ applied.fileCount }} 个文件 / {{ applied.spanCount }} 处
      <button type="button" :disabled="busy" data-testid="rewrite-rollback" @click="doRollback">回滚</button>
    </p>
    <p v-if="error" class="kp-rewrite__error" data-testid="rewrite-error">{{ error }}</p>
  </div>
</template>

<style scoped>
.kp-rewrite { display: flex; flex-direction: column; gap: 6px; }
.kp-rewrite__error { color: #b00020; }
</style>
