<script setup lang="ts">
import { computed, ref } from 'vue';
import { SORT_OPTIONS, EMPTY_HINTS, type SearchSort } from '../logic';
import { useSearchStore } from '../stores/search';
import SearchResultItem from './SearchResultItem.vue';

/**
 * 搜索面板（FR-SEARCH-01~11）。
 *
 * 键盘：↑/↓ 移动选中项、Enter 打开选中项、Esc 关闭面板；输入防抖与"取消上一次"在 store 里。
 * **导航不在这里**：本组件只 emit `open(relPath, line)`，由 WorkspaceLayout 交给
 * `editor.reveal`（与反链面板同一条既有通路）。
 */
const store = useSearchStore();
const emit = defineEmits<{ open: [relPath: string, line: number]; close: [] }>();
const input = ref<HTMLInputElement | null>(null);

const errorKind = computed(() => store.error?.kind ?? null);

function onInput(event: Event): void {
  store.setQuery((event.target as HTMLInputElement).value);
}

function onSort(event: Event): void {
  store.setSort((event.target as HTMLSelectElement).value as SearchSort);
}

function onMode(event: Event): void {
  store.setMatchMode((event.target as HTMLSelectElement).value as 'all' | 'any');
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'ArrowDown') store.moveSelection(1);
  else if (event.key === 'ArrowUp') store.moveSelection(-1);
  else if (event.key === 'Enter') {
    const item = store.selectedItem();
    if (item) emit('open', item.relPath, store.hitLineFor(item));
  } else if (event.key === 'Escape') emit('close');
  else return;
  event.preventDefault();
}

function focusInput(): void {
  input.value?.focus();
}

defineExpose({ focusInput });
</script>

<template>
  <section class="kp-search" data-testid="search-panel">
    <div class="kp-search__bar">
      <input
        ref="input"
        class="kp-search__input"
        data-testid="search-input"
        type="search"
        placeholder="搜索全文…（Enter 打开选中项，Esc 关闭）"
        :value="store.query"
        @input="onInput"
        @keydown="onKeydown"
      >
      <select
        class="kp-search__select"
        data-testid="search-sort"
        :value="store.sort"
        aria-label="排序方式"
        @change="onSort"
      >
        <option v-for="opt in SORT_OPTIONS" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <select
        class="kp-search__select"
        data-testid="search-mode"
        :value="store.matchMode"
        aria-label="匹配模式"
        @change="onMode"
      >
        <option value="all">全部词（AND）</option>
        <option value="any">任一词（OR）</option>
      </select>
      <button type="button" data-testid="search-close" @click="emit('close')">关闭</button>
    </div>

    <p v-if="store.error" class="kp-search__error" :data-kind="errorKind" data-testid="search-error">
      <strong>{{ store.error.title }}</strong>：{{ store.error.message }}
    </p>
    <p v-else-if="store.loading" class="kp-search__hint" data-testid="search-loading">搜索中…</p>

    <ul v-if="store.results.length > 0" class="kp-search__list">
      <SearchResultItem
        v-for="(item, i) in store.results"
        :key="item.relPath"
        :item="item"
        :expanded="store.expanded.includes(item.relPath)"
        :selected="i === store.selected"
        @open="(relPath, line) => emit('open', relPath, line)"
        @toggle="(relPath) => store.toggleExpand(relPath)"
      />
    </ul>

    <div v-if="store.isEmpty" class="kp-search__empty" data-testid="search-empty">
      <p>没有命中。可以试试：</p>
      <ul>
        <li v-for="(hint, i) in EMPTY_HINTS" :key="i">{{ hint }}</li>
      </ul>
    </div>

    <p class="kp-search__total" data-testid="search-total">
      <span v-if="store.searched">共 {{ store.total }} 条</span>
      <button
        v-if="store.hasMore"
        type="button"
        data-testid="search-more"
        :disabled="store.loading"
        @click="store.loadMore()"
      >
        加载更多
      </button>
    </p>
  </section>
</template>

<style scoped>
.kp-search { width: 380px; min-width: 300px; display: flex; flex-direction: column; height: 100%; border-left: 1px solid var(--kp-border, #ddd); overflow: hidden; }
.kp-search__bar { display: flex; gap: 6px; padding: 6px; border-bottom: 1px solid var(--kp-border, #eee); }
.kp-search__input { flex: 1; min-width: 0; }
.kp-search__list { flex: 1; overflow: auto; margin: 0; padding: 0; list-style: none; }
.kp-search__error { margin: 6px 8px; padding: 6px 8px; font-size: 12px; border-left: 3px solid #b00020; background: #fdecea; }
.kp-search__error[data-kind='too-broad'] { border-left-color: #b26a00; background: #fff4e5; }
.kp-search__error[data-kind='syntax'] { border-left-color: #b00020; background: #fdecea; }
.kp-search__hint, .kp-search__total { margin: 6px 8px; font-size: 12px; color: #666; }
.kp-search__empty { margin: 8px; font-size: 12px; color: #666; }
.kp-search__empty ul { margin: 4px 0 0; padding-left: 18px; }
</style>
