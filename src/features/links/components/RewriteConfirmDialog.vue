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
// 显式写成 `| undefined`：项目开了 exactOptionalPropertyTypes，调用方会传 string | undefined
const props = defineProps<{ renameFrom?: string | undefined; renameTo?: string | undefined }>();

const fromRef = ref('');
const toRef = ref('');
const preview = ref<RewritePreview | null>(null);
const applied = ref<{ operationId: string; fileCount: number; spanCount: number } | null>(null);
const error = ref<string | null>(null);
const busy = ref(false);

/** 是否同时改名（勾上之后，改名与改写是**同一次可回滚操作**，FR-FILE-22）。 */
const alsoRename = ref(false);
/** 新文件名（含 .md）。 */
const renameToInput = ref('');
/** 新名字来源：优先 prop（调用方已知），否则用对话框里输入的。 */
const renameTarget = computed(() => props.renameTo ?? (alsoRename.value ? renameToInput.value.trim() : ''));
/** 被跳过文件数。
 *
 * 用 `?? []` 而不是 `preview.skipped.length`：**序列化边界要能容忍字段缺失** ——
 * R3 上线时，一个更早写的、mock 里没带 skipped 的用例（H3 那条）因此在 CI 上运行期崩了，
 * 而本地因为验证发生在重基之前没发现。UI 不该依赖"对端一定给了字段"。
 */
const skippedCount = computed(() => (preview.value?.skipped ?? []).length);

const renameSpec = computed(() =>
  props.renameFrom && renameTarget.value
    ? { from: props.renameFrom, to: renameTarget.value }
    : undefined,
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

    <label v-if="props.renameFrom" class="kp-rewrite__rename">
      <input v-model="alsoRename" type="checkbox" data-testid="rewrite-also-rename">
      同时把 <code>{{ props.renameFrom }}</code> 改名为
      <input v-model="renameToInput" data-testid="rewrite-rename-to" placeholder="新文件名（含 .md）">
    </label>
    <p v-if="renameSpec" class="kp-rewrite__hint" data-testid="rewrite-rename-hint">
      将按 FR-FILE-22 把改名与链接改写合并为**同一次可回滚操作**
    </p>

    <p v-if="preview" data-testid="rewrite-preview-summary">
      将修改 {{ preview.fileCount }} 个文件中的 {{ preview.spanCount }} 处链接
    </p>
    <ul v-if="preview" data-testid="rewrite-preview-items">
      <li v-for="e in preview.edits" :key="e.relPath">
        {{ e.relPath }}（{{ e.hits }} 处）
        <ul v-if="e.occurrences.length" data-testid="rewrite-preview-diff">
          <li v-for="(o, i) in e.occurrences" :key="e.relPath + '-' + i">
            第 {{ o.line }} 行：<code>{{ o.before }}</code> → <code>{{ o.after }}</code>
          </li>
        </ul>
      </li>
    </ul>
    <div v-if="skippedCount" class="kp-rewrite__warn" data-testid="rewrite-skipped">
      <p>有 {{ skippedCount }} 个文件**不会被处理**（读取失败或非文本）：</p>
      <ul>
        <li v-for="s in preview?.skipped ?? []" :key="s">{{ s }}</li>
      </ul>
      <p>它们里面指向旧名的链接会保持原样，改名后会变成悬空链接。</p>
    </div>
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
.kp-rewrite__warn { background: #fff7e6; border: 1px solid #ffd591; padding: 6px 8px; }
</style>
