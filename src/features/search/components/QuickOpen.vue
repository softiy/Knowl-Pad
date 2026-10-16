<script setup lang="ts">
import { ref } from 'vue';
import { useSearchStore } from '../stores/search';

/**
 * 快速打开（FR-SEARCH-40/41，Mod+O）：
 * **匹配一律走后端 `search_quick_open`**（项目所有者裁定：后端是唯一真相；fuse.js 只服务命令面板）。
 * 键盘：↑/↓ 选择、Enter 打开、Esc 关闭。
 */
const store = useSearchStore();
const emit = defineEmits<{ open: [relPath: string]; close: [] }>();
const input = ref<HTMLInputElement | null>(null);

function onInput(event: Event): void {
  store.setQuickQuery((event.target as HTMLInputElement).value);
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'ArrowDown') store.moveQuickSelection(1);
  else if (event.key === 'ArrowUp') store.moveQuickSelection(-1);
  else if (event.key === 'Enter') {
    const item = store.selectedQuick();
    if (item) emit('open', item.relPath);
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
  <div class="kp-quick" data-testid="quick-open" @click.self="emit('close')">
    <div class="kp-quick__box">
      <input
        ref="input"
        class="kp-quick__input"
        data-testid="quick-input"
        type="text"
        placeholder="按文件名或路径片段打开…（例：work/rep）"
        :value="store.quickQuery"
        @input="onInput"
        @keydown="onKeydown"
      >
      <ul class="kp-quick__list">
        <li
          v-for="(item, i) in store.quickItems"
          :key="item.relPath"
          class="kp-quick__item"
          :class="{ 'kp-quick__item--selected': i === store.quickSelected }"
          data-testid="quick-item"
          @click="emit('open', item.relPath)"
        >
          <span class="kp-quick__name">{{ item.name }}</span>
          <span class="kp-quick__path">{{ item.relPath }}</span>
        </li>
      </ul>
      <p v-if="store.quickItems.length === 0" class="kp-quick__empty" data-testid="quick-empty">
        没有匹配的文件。
      </p>
    </div>
  </div>
</template>

<style scoped>
.kp-quick { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.25); display: flex; justify-content: center; padding-top: 12vh; z-index: 20; }
.kp-quick__box { width: min(560px, 90vw); max-height: 60vh; display: flex; flex-direction: column; background: var(--kp-bg, #fff); border-radius: 6px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.25); overflow: hidden; }
.kp-quick__input { margin: 8px; padding: 6px 8px; font-size: 14px; }
.kp-quick__list { flex: 1; overflow: auto; margin: 0; padding: 0; list-style: none; }
.kp-quick__item { display: flex; gap: 8px; padding: 6px 10px; cursor: pointer; font-size: 13px; }
.kp-quick__item--selected { background: var(--kp-selected, #eef4ff); }
.kp-quick__name { font-weight: 600; }
.kp-quick__path { color: #777; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.kp-quick__empty { margin: 8px 10px; font-size: 12px; color: #777; }
</style>
