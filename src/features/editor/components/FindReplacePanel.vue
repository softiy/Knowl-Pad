<script setup lang="ts">
/**
 * 文件内查找替换面板（FR-SEARCH-30/31；FR-EDITOR-09 的细化）。
 *
 * 三处刻意：
 * ① 查找/替换一律经**适配器**（§7.4），换内核（M8）时本文件一行都不用改；
 * ② 跳转复用既有的 `revealLine(line, range)` 通路（与搜索/反链同一条），不新造定位机制；
 * ③ 输入错误与替换失败**必须可见**（R-15）；空查询只清空计数，不算错误。
 */
import { computed, nextTick, ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import type { InFileMatch, KpEditorAdapter } from '../adapter';

const props = withDefaults(defineProps<{ adapter: KpEditorAdapter | null; content?: string; mode?: 'find' | 'replace' }>(), {
  content: '',
  mode: 'find',
});
const emit = defineEmits<{ close: [] }>();

const query = ref('');
const replacement = ref('');
const caseSensitive = ref(false);
const wholeWord = ref(false);
const useRegex = ref(false);
const matches = ref<InFileMatch[]>([]);
const current = ref(-1);
const error = ref<string | null>(null);
const queryInput = ref<HTMLInputElement | null>(null);
const replacementInput = ref<HTMLInputElement | null>(null);

const isEmptyQuery = computed(() => query.value.length === 0);
const countText = computed(() => {
  if (isEmptyQuery.value) return '—';
  if (matches.value.length === 0) return '共 0 处';
  return `第 ${current.value + 1} / 共 ${matches.value.length} 处`;
});

function options(): { caseSensitive: boolean; wholeWord: boolean; regex: boolean } {
  return { caseSensitive: caseSensitive.value, wholeWord: wholeWord.value, regex: useRegex.value };
}

/** 偏移（UTF-16）→ 1-based 行号 + 行内列区间（revealLine 的口径）。 */
function lineRangeOf(match: InFileMatch): { line: number; range: { start: number; end: number } } {
  const before = props.content.slice(0, match.from);
  const line = before.split('\n').length;
  const lineStart = before.lastIndexOf('\n') + 1;
  const start = match.from - lineStart;
  return { line, range: { start, end: start + (match.to - match.from) } };
}

/** 定位并选中第 index 条（环绕；空列表返回 false）。 */
function goto(index: number): boolean {
  if (!props.adapter || matches.value.length === 0) return false;
  const size = matches.value.length;
  const next = ((index % size) + size) % size;
  const target = matches.value[next];
  if (!target) return false;
  const { line, range } = lineRangeOf(target);
  props.adapter.revealLine(line, range);
  current.value = next;
  error.value = null;
  return true;
}

/** 重新查找并定位到第一条（输入即查）。 */
function runFind(): void {
  if (!props.adapter) return;
  if (isEmptyQuery.value) {
    matches.value = [];
    current.value = -1;
    error.value = null;
    return;
  }
  try {
    matches.value = props.adapter.find(query.value, options());
    current.value = matches.value.length > 0 ? 0 : -1;
    error.value = null;
    goto(0);
  } catch (err) {
    matches.value = [];
    current.value = -1;
    error.value = asKpError(err).message;
  }
}

function next(): void {
  if (matches.value.length === 0) return;
  goto(current.value + 1);
}
function prev(): void {
  if (matches.value.length === 0) return;
  goto(current.value - 1);
}

/** 逐条替换：只替换当前一条，然后重新查找并保持位置。 */
function replaceOne(): void {
  if (!props.adapter || current.value < 0) return;
  const target = matches.value[current.value];
  if (!target) return;
  try {
    props.adapter.replace([target], replacement.value);
    runFind();
    // 替换后命中列表整体位移，尽量停在原位
    if (matches.value.length > 0) goto(Math.min(current.value, matches.value.length - 1));
  } catch (err) {
    error.value = asKpError(err).message;
  }
}

/** 全部替换：**没有命中时绝不调用 replace**（不写任何东西）。 */
function replaceAll(): void {
  if (!props.adapter || isEmptyQuery.value) return;
  try {
    const all = props.adapter.find(query.value, options());
    if (all.length === 0) {
      matches.value = [];
      current.value = -1;
      error.value = null;
      return;
    }
    props.adapter.replace(all, replacement.value);
    runFind();
  } catch (err) {
    error.value = asKpError(err).message;
  }
}

function onQueryKeydown(event: KeyboardEvent): void {
  if (event.key === 'Enter') {
    event.preventDefault();
    if (event.shiftKey) prev();
    else next();
  } else if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
  }
}

/** 打开时聚焦：查找聚焦查询框；替换模式聚焦替换框（Ctrl+H 的直觉）。 */
function focusInput(): void {
  void nextTick(() => {
    if (props.mode === 'replace') replacementInput.value?.focus();
    else queryInput.value?.focus();
  });
}
defineExpose({ focusInput });
</script>

<template>
  <div class="kp-find" data-testid="find-panel">
    <input ref="queryInput" v-model="query" data-testid="find-query" placeholder="查找" aria-label="查找" @input="runFind" @keydown="onQueryKeydown">
    <button type="button" data-testid="find-prev" title="上一处（Shift+Enter）" :disabled="matches.length === 0" @click="prev">↑</button>
    <button type="button" data-testid="find-next" title="下一处（Enter）" :disabled="matches.length === 0" @click="next">↓</button>
    <span class="kp-find__hint" data-testid="find-count" aria-live="polite">{{ countText }}</span>
    <template v-if="mode === 'replace'">
      <input ref="replacementInput" v-model="replacement" data-testid="find-replacement" placeholder="替换为" aria-label="替换为">
      <button type="button" data-testid="replace-one" :disabled="current < 0" @click="replaceOne">替换</button>
      <button type="button" data-testid="replace-all" :disabled="isEmptyQuery" @click="replaceAll">全部替换</button>
    </template>
    <label><input v-model="caseSensitive" type="checkbox" @change="runFind">区分大小写</label>
    <label><input v-model="wholeWord" type="checkbox" @change="runFind">全字匹配</label>
    <label><input v-model="useRegex" type="checkbox" @change="runFind">正则</label>
    <button type="button" data-testid="find-close" title="关闭（Esc）" @click="emit('close')">✕</button>
    <span v-if="error" class="kp-find__error" data-testid="find-error" role="alert">{{ error }}</span>
  </div>
</template>

<style scoped>
.kp-find { display: flex; align-items: center; gap: 8px; padding: 4px 8px; border-bottom: 1px solid var(--kp-border, #ddd); font-size: 12px; }
.kp-find input[type='text'], .kp-find input:not([type]) { min-width: 120px; }
.kp-find__hint { color: var(--kp-muted, #666); }
.kp-find__error { color: #b00020; }
</style>
