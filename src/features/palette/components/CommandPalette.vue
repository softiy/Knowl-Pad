<script setup lang="ts">
/**
 * 命令面板（FR-PALETTE-01/05/06/07；AC-PALETTE-01/02 的机制面）。
 *
 * 只做渲染与键盘事件转发：状态与执行在 `usePaletteStore`（快捷键在 WorkspaceLayout 统一绑定）。
 * 为什么键盘处理放在输入框上：焦点始终在输入框（输入即过滤），方向键/Enter/Esc 必须在此拦截，
 * 否则会落到编辑器的按键处理里（ED-04 的同一个理由）。
 */
import { computed, nextTick, ref, watch } from 'vue';
import { storeToRefs } from 'pinia';
import { disabledReason, isEnabled } from '../registry';
import { usePaletteStore } from '../store';

const palette = usePaletteStore();
const { isOpen, query, activeIndex, error, results, context, activeCommand } = storeToRefs(palette);
const inputRef = ref<HTMLInputElement | null>(null);

const activeId = computed<string>(() => `palette-item-${activeIndex.value}`);

watch(isOpen, (open) => {
  if (open) void nextTick(() => inputRef.value?.focus());
});

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'ArrowDown') {
    event.preventDefault();
    palette.move(1);
  } else if (event.key === 'ArrowUp') {
    event.preventDefault();
    palette.move(-1);
  } else if (event.key === 'Enter') {
    event.preventDefault();
    void palette.execute();
  } else if (event.key === 'Escape') {
    event.preventDefault();
    palette.close();
  }
}

function enabled(command: (typeof results.value)[number]): boolean {
  return isEnabled(command, context.value);
}

function reason(command: (typeof results.value)[number]): string {
  return enabled(command) ? '' : disabledReason(command, context.value);
}
</script>

<template>
  <div v-if="isOpen" class="kp-palette" data-testid="palette" @mousedown.self="palette.close()">
    <div class="kp-palette__panel" role="dialog" aria-modal="true" aria-label="命令面板">
      <input
        ref="inputRef"
        class="kp-palette__input"
        data-testid="palette-input"
        type="text"
        placeholder="输入命令名（支持拼音首字母，如 xj）"
        :value="query"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        :aria-activedescendant="activeId"
        @input="palette.setQuery(($event.target as HTMLInputElement).value)"
        @keydown="onKeydown"
      >
      <ul id="palette-list" class="kp-palette__list" role="listbox" data-testid="palette-list">
        <li
          v-for="(command, index) in results"
          :id="`palette-item-${index}`"
          :key="command.id"
          class="kp-palette__item"
          :class="{ 'kp-palette__item--active': index === activeIndex, 'kp-palette__item--disabled': !enabled(command) }"
          role="option"
          :aria-selected="index === activeIndex"
          :aria-disabled="!enabled(command)"
          :data-testid="`palette-item-${command.id}`"
          :title="reason(command)"
          @mousedown.prevent="palette.execute(index)"
        >
          <span class="kp-palette__icon" aria-hidden="true">{{ command.icon }}</span>
          <span class="kp-palette__name">{{ command.name }}</span>
          <span v-if="!enabled(command)" class="kp-palette__reason">{{ reason(command) }}</span>
          <span v-else-if="command.keys" class="kp-palette__keys">{{ command.keys }}</span>
        </li>
        <li v-if="results.length === 0" class="kp-palette__empty" data-testid="palette-empty">
          没有匹配的命令
        </li>
      </ul>
      <p v-if="error" class="kp-palette__error" data-testid="palette-error" role="alert">{{ error }}</p>
      <p class="kp-palette__hint" data-testid="palette-hint">
        ↑↓ 选择 · Enter 执行 · Esc 关闭<span v-if="activeCommand"> · 当前：{{ activeCommand.name }}</span>
      </p>
    </div>
  </div>
</template>

<style scoped>
.kp-palette { position: fixed; inset: 0; z-index: 1000; display: flex; align-items: flex-start; justify-content: center; padding-top: 12vh; background: rgba(0, 0, 0, 0.35); }
.kp-palette__panel { width: min(640px, 92vw); max-height: 60vh; display: flex; flex-direction: column; background: var(--kp-bg, #fff); border: 1px solid var(--kp-border, #ccc); border-radius: 8px; box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25); overflow: hidden; }
.kp-palette__input { width: 100%; box-sizing: border-box; padding: 10px 12px; font-size: 14px; border: 0; border-bottom: 1px solid var(--kp-border, #ddd); outline: none; }
.kp-palette__list { list-style: none; margin: 0; padding: 4px 0; overflow-y: auto; }
.kp-palette__item { display: flex; align-items: center; gap: 8px; padding: 6px 12px; font-size: 13px; cursor: pointer; }
.kp-palette__item--active { background: var(--kp-accent-weak, #e8f0fe); }
.kp-palette__item--disabled { opacity: 0.45; cursor: not-allowed; }
.kp-palette__icon { width: 18px; text-align: center; }
.kp-palette__name { flex: 1; }
.kp-palette__keys { color: var(--kp-muted, #888); font-size: 11px; font-variant-numeric: tabular-nums; }
.kp-palette__reason { color: var(--kp-muted, #888); font-size: 11px; }
.kp-palette__empty { padding: 10px 12px; font-size: 12px; color: var(--kp-muted, #888); }
.kp-palette__error { margin: 0; padding: 6px 12px; font-size: 12px; color: #b3261e; background: #fdecea; }
.kp-palette__hint { margin: 0; padding: 6px 12px; font-size: 11px; color: var(--kp-muted, #888); border-top: 1px solid var(--kp-border, #eee); }
</style>
