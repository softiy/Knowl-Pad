<script setup lang="ts">
import { computed } from 'vue';
import type { SearchResultItem, SearchSnippet } from '@core/ipc/searchCommands';
import { formatMtime, hitCountLabel, splitHighlight, visibleSnippets } from '../logic';

/**
 * 一条搜索结果（FR-SEARCH-04/05）：笔记名 / 相对路径 / 命中片段（含上下文与命中高亮）/
 * 修改时间 / 命中次数，多处命中可展开。
 *
 * 合并与计数**都来自后端**（item.hitCount 是后端算的）：本组件只决定"显示几条片段"。
 */
const props = defineProps<{
  item: SearchResultItem;
  expanded: boolean;
  selected: boolean;
}>();

const emit = defineEmits<{
  open: [relPath: string, line: number, range?: { start: number; end: number }];
  toggle: [relPath: string];
}>();

/** 首个片段的命中位置（行 + 列区间）：行用来滚动，区间用来精确选中。 */
function firstHit(): { line: number; range?: { start: number; end: number } } {
  const s = props.item.snippets[0];
  return s
    ? { line: s.highlightLine, range: { start: s.highlightStart, end: s.highlightEnd } }
    : { line: 1 };
}

const snippets = computed(() => visibleSnippets(props.item, props.expanded));
const canExpand = computed(() => props.item.snippets.length > 1);
const hitLabel = computed(() => hitCountLabel(props.item));

/** 命中行的三段切分；非命中行原样返回。 */
function segments(snippet: SearchSnippet, lineNo: number, text: string) {
  if (lineNo !== snippet.highlightLine) return [{ text, hit: false }];
  const { before, hit, after } = splitHighlight(text, snippet.highlightStart, snippet.highlightEnd);
  return [
    { text: before, hit: false },
    { text: hit, hit: true },
    { text: after, hit: false },
  ].filter((s) => s.text.length > 0);
}
</script>

<template>
  <li
    class="kp-hit"
    :class="{ 'kp-hit--selected': props.selected }"
    data-testid="search-result"
    @click="emit('open', props.item.relPath, firstHit().line, firstHit().range)"
  >
    <div class="kp-hit__head">
      <span class="kp-hit__name" data-testid="search-result-name">{{ props.item.name }}</span>
      <span class="kp-hit__path" data-testid="search-result-path">{{ props.item.relPath }}</span>
      <span class="kp-hit__mtime">{{ formatMtime(props.item.mtimeMs) }}</span>
      <span class="kp-hit__count" data-testid="search-result-count">{{ hitLabel }}</span>
    </div>

    <div v-for="(snippet, si) in snippets" :key="si" class="kp-hit__snippet">
      <div v-for="line in snippet.lines" :key="line.line" class="kp-hit__line">
        <span class="kp-hit__lineno">{{ line.line }}</span>
        <span class="kp-hit__text">
          <template v-for="(seg, gi) in segments(snippet, line.line, line.text)" :key="gi">
            <mark v-if="seg.hit" class="kp-hit__mark" data-testid="search-hit-mark">{{ seg.text }}</mark>
            <template v-else>{{ seg.text }}</template>
          </template>
        </span>
      </div>
    </div>

    <button
      v-if="canExpand"
      type="button"
      class="kp-hit__expand"
      data-testid="search-result-expand"
      @click.stop="emit('toggle', props.item.relPath)"
    >
      {{ props.expanded ? '收起其余命中' : `展开其余 ${props.item.snippets.length - 1} 处` }}
    </button>
  </li>
</template>

<style scoped>
.kp-hit { padding: 6px 8px; border-bottom: 1px solid var(--kp-border, #eee); cursor: pointer; }
.kp-hit--selected { background: var(--kp-selected, #eef4ff); }
.kp-hit__head { display: flex; gap: 8px; align-items: baseline; font-size: 12px; }
.kp-hit__name { font-weight: 600; }
.kp-hit__path { color: #666; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.kp-hit__mtime, .kp-hit__count { color: #888; white-space: nowrap; }
.kp-hit__snippet { margin-top: 4px; font-family: var(--kp-mono, monospace); font-size: 12px; }
.kp-hit__line { display: flex; gap: 6px; }
.kp-hit__lineno { color: #aaa; min-width: 2.5em; text-align: right; user-select: none; }
.kp-hit__mark { background: #ffe58f; padding: 0 1px; }
.kp-hit__expand { margin-top: 4px; font-size: 11px; }
</style>
