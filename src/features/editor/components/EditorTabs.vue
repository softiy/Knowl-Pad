<script setup lang="ts">
import { ref } from 'vue';
import { useEditorStore } from '../stores/editor';

/** 多标签栏（FR-EDITOR-35 的基础部分：打开/切换/关闭 + 未保存标记）。 */
const store = useEditorStore();
/** 待确认关闭的标签（FR-EDITOR-32：关闭有未保存内容的标签前必须提示）。 */
const pendingClose = ref<string | null>(null);

function requestClose(relPath: string): void {
  if (store.isDirty(relPath)) {
    pendingClose.value = relPath;
    return;
  }
  store.closeNote(relPath);
}

function confirmClose(save: boolean): void {
  const relPath = pendingClose.value;
  pendingClose.value = null;
  if (!relPath) return;
  if (save) {
    void store.save(relPath).then(() => store.closeNote(relPath));
    return;
  }
  store.closeNote(relPath);
}

function baseName(relPath: string): string {
  return relPath.split('/').pop() ?? relPath;
}
</script>

<template>
  <div class="kp-tabs">
    <div class="kp-tabs__list" role="tablist">
      <button
        v-for="buffer in store.buffers"
        :key="buffer.relPath"
        type="button"
        role="tab"
        class="kp-tabs__tab"
        :class="{ 'kp-tabs__tab--active': buffer.relPath === store.activeRelPath }"
        :data-testid="`tab-${buffer.relPath}`"
        :aria-selected="buffer.relPath === store.activeRelPath"
        @click="store.activate(buffer.relPath)"
      >
        <span class="kp-tabs__name">{{ baseName(buffer.relPath) }}</span>
        <span v-if="store.isDirty(buffer.relPath)" class="kp-tabs__dirty" data-testid="dirty-dot">●</span>
        <span class="kp-tabs__close" data-testid="tab-close" @click.stop="requestClose(buffer.relPath)">×</span>
      </button>
    </div>
    <div v-if="pendingClose" class="kp-tabs__confirm" data-testid="close-confirm">
      <span>「{{ baseName(pendingClose) }}」有未保存的修改：</span>
      <button type="button" @click="confirmClose(true)">保存并关闭</button>
      <button type="button" @click="confirmClose(false)">放弃修改</button>
      <button type="button" @click="pendingClose = null">取消</button>
    </div>
  </div>
</template>

<style scoped>
.kp-tabs { border-bottom: 1px solid var(--kp-border, #ddd); }
.kp-tabs__list { display: flex; gap: 2px; overflow-x: auto; }
.kp-tabs__tab { display: inline-flex; align-items: center; gap: 4px; padding: 4px 8px; border: 0; background: none; cursor: pointer; font-size: 13px; white-space: nowrap; }
.kp-tabs__tab--active { background: var(--kp-tab-active, #eef2f7); font-weight: 600; }
.kp-tabs__dirty { color: #b26a00; font-size: 10px; }
.kp-tabs__close { opacity: 0.6; }
.kp-tabs__close:hover { opacity: 1; }
.kp-tabs__confirm { display: flex; gap: 8px; align-items: center; padding: 4px 8px; background: #fff8e1; font-size: 12px; }
</style>
