<script setup lang="ts">
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import type { KpEditorAdapter } from '../adapter';

/**
 * 文件内查找替换面板（FR-EDITOR-09）。
 *
 * 从 `EditorView` 抽出来的（CODE-11 文件长度：EditorView 当时 205 SLOC，预警线 200）——
 * 抽出的理由是它本来就是一个**自足的小面板**：状态、校验、按钮文案都只服务这一件事，
 * 与编辑器主视图的联动只有"把适配器给我"。
 *
 * 两处刻意：
 * ① 查找/替换一律经 **适配器**（§7.4）—— 换内核（M8）时本文件一行都不用改；
 * ② 无效正则等输入错误**必须可见**（R-15 / ERR-02）：空查询不算错误，只清空计数。
 */
const props = defineProps<{ adapter: KpEditorAdapter | null }>();

const query = ref('');
const replacement = ref('');
const caseSensitive = ref(false);
const wholeWord = ref(false);
const useRegex = ref(false);
const matchCount = ref<number | null>(null);
const error = ref<string | null>(null);

function options(): { caseSensitive: boolean; wholeWord: boolean; regex: boolean } {
  return { caseSensitive: caseSensitive.value, wholeWord: wholeWord.value, regex: useRegex.value };
}

/** 执行查找并更新命中数（空查询不视为错误，只清空计数）。 */
function runFind(): void {
  if (!props.adapter) return;
  if (!query.value) {
    matchCount.value = null;
    error.value = null;
    return;
  }
  try {
    matchCount.value = props.adapter.find(query.value, options()).length;
    error.value = null;
  } catch (err) {
    // 无效正则等输入错误：给出可见提示，绝不让面板静默失效
    matchCount.value = null;
    error.value = asKpError(err).message;
  }
}

/** 全部替换（FR-EDITOR-09 允许「逐条确认或全部替换」，此处实现全部替换并回显剩余命中数）。 */
function replaceAll(): void {
  if (!props.adapter || !query.value) return;
  const matches = props.adapter.find(query.value, options());
  if (matches.length === 0) {
    matchCount.value = 0;
    return;
  }
  props.adapter.replace(matches, replacement.value);
  matchCount.value = props.adapter.find(query.value, options()).length;
}
</script>

<template>
  <div class="kp-find" data-testid="find-panel">
    <input v-model="query" data-testid="find-query" placeholder="查找" @input="runFind">
    <input v-model="replacement" data-testid="find-replacement" placeholder="替换为">
    <label><input v-model="caseSensitive" type="checkbox" @change="runFind">区分大小写</label>
    <label><input v-model="wholeWord" type="checkbox" @change="runFind">全字匹配</label>
    <label><input v-model="useRegex" type="checkbox" @change="runFind">正则</label>
    <button type="button" data-testid="find-run" @click="runFind">查找</button>
    <button type="button" data-testid="replace-all" :disabled="!query" @click="replaceAll">全部替换</button>
    <span class="kp-find__hint" data-testid="find-count">{{ matchCount === null ? '—' : matchCount }} 处</span>
    <span v-if="error" class="kp-find__error" data-testid="find-error">{{ error }}</span>
  </div>
</template>

<style scoped>
.kp-find { display: flex; align-items: center; gap: 8px; padding: 4px 8px; border-bottom: 1px solid var(--kp-border, #ddd); font-size: 12px; }
.kp-find input[type='text'], .kp-find input:not([type]) { min-width: 120px; }
.kp-find__hint { color: #666; }
.kp-find__error { color: #b00020; }
</style>
