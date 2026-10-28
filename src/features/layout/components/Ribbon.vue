<script setup lang="ts">
/**
 * Ribbon（FR-LAYOUT-02）：只放**真实存在**的入口 —— 文件树、搜索、标签、设置。
 * 图谱 / 回收站视图在 M5 不存在，按 D-44 同口径登记去向（不造占位按钮）。
 */
const props = defineProps<{ collapsed: boolean; rightPanelVisible: boolean }>();
const emit = defineEmits<{
  toggleSidebar: [];
  search: [];
  tags: [];
  settings: [];
  toggleRight: [];
}>();
</script>

<template>
  <nav class="kp-ribbon" data-testid="ribbon" aria-label="快速入口">
    <button type="button" class="kp-ribbon__btn" data-testid="ribbon-files" :aria-pressed="!props.collapsed" title="文件树（折叠/展开左侧栏）" @click="emit('toggleSidebar')">▤</button>
    <button type="button" class="kp-ribbon__btn" data-testid="ribbon-search" title="搜索（Mod+Shift+F）" @click="emit('search')">⌕</button>
    <button type="button" class="kp-ribbon__btn" data-testid="ribbon-tags" title="标签（Mod+Shift+T）" @click="emit('tags')">＃</button>
    <button type="button" class="kp-ribbon__btn" data-testid="ribbon-right" :aria-pressed="props.rightPanelVisible" title="右侧面板（反链）" @click="emit('toggleRight')">▥</button>
    <button type="button" class="kp-ribbon__btn" data-testid="ribbon-settings" title="设置" @click="emit('settings')">⚙</button>
  </nav>
</template>

<style scoped>
.kp-ribbon { width: 40px; flex: 0 0 40px; display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 6px 0; border-right: 1px solid var(--kp-border); background: var(--kp-bg); }
.kp-ribbon__btn { width: 28px; height: 28px; font-size: 14px; line-height: 1; border: 1px solid transparent; border-radius: 4px; background: transparent; color: var(--kp-fg); cursor: pointer; }
.kp-ribbon__btn:hover { background: var(--kp-accent-weak); }
.kp-ribbon__btn[aria-pressed='true'] { border-color: var(--kp-border); background: var(--kp-accent-weak); }
</style>
