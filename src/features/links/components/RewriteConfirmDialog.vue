<script setup lang="ts">
import { computed, ref } from 'vue';
import RewriteConfirmPanel from '@shared/components/RewriteConfirmPanel.vue';
import { linkRewriteApply, linkRewritePreview, linkRewriteRollback } from '@core/ipc/linkCommands';
import type { RewritePreview, RewriteResult } from '@core/ipc/linkCommands';


/**
 * 链接域的批量改写确认（FR-FILE-21/22）。
 *
 * **W7 起是薄容器**："预览 → 确认 → 回滚"的骨架在 `@shared/components/RewriteConfirmPanel.vue`（域无关），
 * 本文件只负责两件**链接域特有**的事：
 * ① 把这三个动作接到 `link_rewrite_*` 命令上；
 * ② 提供"同时改名"输入区（`rename` 规格），改名与改写属**同一次可回滚操作**（FR-FILE-22）——
 *    此时不要再调用 `file_rename`：PRD §5.3.3 明确 `file_rename`「不含链接改写」。
 */
// 显式写成 `| undefined`：项目开了 exactOptionalPropertyTypes，调用方会传 string | undefined
const props = defineProps<{ renameFrom?: string | undefined; renameTo?: string | undefined }>();

/** 是否同时改名（勾上后改名与改写是**同一次可回滚操作**，FR-FILE-22）。 */
const alsoRename = ref(false);
/** 新文件名（含 .md）。 */
const renameToInput = ref('');

/** 新名字来源：优先 prop（调用方已知），否则用对话框里输入的。 */
const renameTarget = computed(() => props.renameTo ?? (alsoRename.value ? renameToInput.value.trim() : ''));
const renameSpec = computed(() =>
  props.renameFrom && renameTarget.value
    ? { from: props.renameFrom, to: renameTarget.value }
    : undefined,
);
const showRenameHint = computed(() => renameSpec.value !== undefined);

/** 预览结果只读地读一遍（面板持有原件）：两处状态不互抄，避免"谁先更新"的时序问题。 */
function editsOf(raw: unknown): RewritePreview['edits'] {
  return (raw as RewritePreview | null)?.edits ?? [];
}

function onPreview(from: string, to: string): Promise<unknown> {
  return linkRewritePreview(from, to, renameSpec.value);
}

function onApply(_from: string, _to: string, raw: unknown): Promise<RewriteResult> {
  return linkRewriteApply((raw as RewritePreview).previewId, renameSpec.value);
}

/** 回滚只需要 operationId（AC-FILE-02）—— 它是**本次执行**的凭据，不在预览里。
 *
 * 模板里的 `$event` 由本函数的**声明返回类型**决定（`RewriteResult`），
 * 因此这里不需要任何 `as`：`operationId` 是它的一部分。
 */
function onRollback(result: RewriteResult): Promise<void> {
  return linkRewriteRollback(result.operationId);
}
</script>

<template>
  <RewriteConfirmPanel
    :on-preview="onPreview"
    :on-apply="onApply"
    :on-rollback="onRollback"
    unit-label="链接"
  >
    <template #inputs="{ busy }">
      <label v-if="props.renameFrom" class="kp-rewrite__rename">
        <input v-model="alsoRename" type="checkbox" data-testid="rewrite-also-rename">
        同时把 <code>{{ props.renameFrom }}</code> 改名为
        <input
          v-model="renameToInput"
          data-testid="rewrite-rename-to"
          placeholder="新文件名（含 .md）"
          :disabled="busy"
        >
      </label>
      <p v-if="showRenameHint" class="kp-rewrite__hint" data-testid="rewrite-rename-hint">
        将按 FR-FILE-22 把改名与链接改写合并为**同一次可回滚操作**
      </p>
    </template>

    <template #preview-items="{ preview }">
      <li v-for="e in editsOf(preview)" :key="e.relPath">
        {{ e.relPath }}（{{ e.hits }} 处）
        <ul v-if="e.occurrences.length" data-testid="rewrite-preview-diff">
          <li v-for="(o, i) in e.occurrences" :key="e.relPath + '-' + i">
            第 {{ o.line }} 行：<code>{{ o.before }}</code> → <code>{{ o.after }}</code>
          </li>
        </ul>
      </li>
    </template>
  </RewriteConfirmPanel>
</template>

<style scoped>
.kp-rewrite__rename { display: block; }
.kp-rewrite__hint { color: #666; font-size: 12px; }
</style>
