<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import TagNotesList from './TagNotesList.vue';
import TagRenameDialog from './TagRenameDialog.vue';
import TagTree from './TagTree.vue';
import { useTagsStore } from '../stores/tags';
import { TAG_SORT_OPTIONS, type TagSortMode } from '../logic';
import type { TagNode } from '@core/ipc/tagCommands';

/**
 * 标签面板（FR-TAG-01~04 / 13）。
 *
 * 三处**刻意**的设计：
 * ① 排序与过滤是**纯函数在前端做**（`visibleTree`/`rows`）：树本身来自索引库实时查询，
 *    切换排序不该再打一次 IPC —— 数据只有一份，"排序口径"也就只有一个地方（`logic.ts`）；
 * ② **不显示"子孙合计"**：祖先的 refCount 天然已含子孙（祖先登记），页面上的数字直接来自索引库；
 * ③ 「含子级」开关**保留但如实标注它是 no-op**（勘误 D-40②）：MD-TAG-03 的祖先登记让
 *    "只含自身"与"含全部子孙"返回同一批笔记。**UI 不许假装它生效**，所以钩子旁边常驻一行说明。
 */
const emit = defineEmits<{
  open: [relPath: string];
  close: [];
}>();

const store = useTagsStore();
const filterInput = ref<HTMLInputElement | null>(null);
/** 「未分类」计数（篇数口径）：与标签计数分开显示，避免把两种单位混成一个数。 */
const unclassifiedNode = computed<TagNode | null>(
  () => store.tree.find((n) => n.unclassified) ?? null,
);

function onSort(event: Event): void {
  store.setSortMode((event.target as HTMLSelectElement).value as TagSortMode);
}

function onFilter(event: Event): void {
  store.setFilter((event.target as HTMLInputElement).value);
}

/** 参数在**调用方**解析：模板的 change 事件给的是 Event，store 不该去猜。 */
function onIncludeChildren(event: Event): void {
  store.setIncludeChildren((event.target as HTMLInputElement).checked);
}

/** 选中会变 ⇒ 拉该标签下的笔记（`select` 只改状态，查询在这里统一发起）。 */
watch(
  () => store.selectedNorm,
  (norm) => {
    if (norm !== null) void store.loadNotes(norm);
  },
);

onMounted(async () => {
  await store.subscribe();
  await store.refresh();
});
onUnmounted(() => store.dispose());

/** 快捷键打开面板后聚焦过滤框（与搜索面板同一形态：容器 `defineExpose`）。 */
function focusFilter(): void {
  void nextTick(() => filterInput.value?.focus());
}

defineExpose({ focusFilter });
</script>

<template>
  <aside class="kp-tags" data-testid="tag-panel">
    <header class="kp-tags__bar">
      <input
        ref="filterInput"
        class="kp-tags__filter"
        data-testid="tag-filter"
        type="search"
        placeholder="过滤标签…"
        :value="store.filter"
        @input="onFilter"
      >
      <select
        class="kp-tags__sort"
        data-testid="tag-sort"
        aria-label="标签排序方式"
        :value="store.sortMode"
        @change="onSort"
      >
        <option v-for="opt in TAG_SORT_OPTIONS" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <button
        type="button"
        data-testid="tag-reload"
        title="重新查询索引库（FR-TAG-12 的重算入口是既有的 index_rebuild，不在面板里）"
        :disabled="store.loading"
        @click="store.reloadActive()"
      >
        刷新
      </button>
      <button type="button" data-testid="tag-panel-close" @click="emit('close')">关闭</button>
    </header>

    <p class="kp-tags__total" data-testid="tag-total">
      共 {{ store.totalTags }} 个标签<span v-if="unclassifiedNode">；未分类 {{ unclassifiedNode.refCount }} 篇</span>
    </p>

    <p v-if="store.error" class="kp-tags__error" data-testid="tag-error">{{ store.error }}</p>
    <p v-else-if="store.loading" class="kp-tags__hint" data-testid="tag-loading">读取标签索引…</p>

    <p v-if="store.hasFilter && store.rows.length === 0" class="kp-tags__hint" data-testid="tag-filter-empty">
      没有匹配「{{ store.filter }}」的标签
    </p>
    <TagTree
      v-else
      :nodes="store.visibleTree"
      :selected-norm="store.selectedNorm"
      @select="(norm) => store.select(norm)"
    />

    <label class="kp-tags__switch">
      <input
        type="checkbox"
        data-testid="tag-include-children"
        :checked="store.includeChildren"
        @change="onIncludeChildren"
      >
      含子级
    </label>
    <p class="kp-tags__hint" data-testid="tag-include-children-hint">
      ⚠️ 当前祖先登记使该开关**无差别**：层级标签同时登记了自身与全部祖先（MD-TAG-03），
      于是"只含自身"与"含全部子孙"返回同一批笔记（勘误 D-40②，批次 4 加 source 标记后才生效）。
    </p>

    <TagNotesList
      :notes="store.notes"
      :active-norm="store.selectedNorm"
      :active-node="store.selectedNode"
      :loading="store.notesLoading"
      @open="(relPath) => emit('open', relPath)"
    />

    <details class="kp-tags__rename">
      <summary>重命名标签…</summary>
      <TagRenameDialog
        :from-tag="store.selectedNode?.norm"
        @renamed="store.reloadActive()"
      />
    </details>
  </aside>
</template>

<style scoped>
.kp-tags { border-left: 1px solid var(--kp-border, #ddd); display: flex; flex-direction: column; gap: 4px; overflow: auto; padding: 8px; width: 340px; }
.kp-tags__bar { display: flex; gap: 4px; flex-wrap: wrap; }
.kp-tags__filter { flex: 1; min-width: 90px; }
.kp-tags__bar button { white-space: nowrap; }
.kp-tags__total { margin: 2px 0; font-size: 12px; color: #666; }
.kp-tags__error { color: #b00020; font-size: 12px; }
.kp-tags__hint { color: #888; font-size: 11px; margin: 2px 0; }
.kp-tags__switch { font-size: 12px; }
.kp-tags__rename { margin-top: 6px; }
</style>
