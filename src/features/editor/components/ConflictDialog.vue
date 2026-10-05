<script setup lang="ts">
import { useEditorStore, type ConflictChoice } from '../stores/editor';

/** 外部修改冲突（FR-EDITOR-34）：必须提供「加载外部版本 / 保留我的版本并覆盖 / 查看差异」，禁止静默覆盖。 */
const store = useEditorStore();
const props = defineProps<{ relPath: string; diff: { type: string; text: string }[] | null }>();
const emit = defineEmits<{ resolve: [choice: ConflictChoice] }>();

function resolve(choice: ConflictChoice): void {
  emit('resolve', choice);
}

function prefixOf(type: string): string {
  if (type === 'added') return '+';
  if (type === 'removed') return '-';
  return ' ';
}

function lineClass(type: string): string {
  if (type === 'added') return 'kp-conflict__line--added';
  if (type === 'removed') return 'kp-conflict__line--removed';
  return 'kp-conflict__line--same';
}
</script>

<template>
  <div class="kp-conflict" data-testid="conflict-bar">
    <p class="kp-conflict__title">文件已被外部修改（{{ props.relPath }}）——请选择处理方式：</p>
    <div class="kp-conflict__actions">
      <button type="button" @click="resolve('loadExternal')">加载外部版本</button>
      <button type="button" @click="resolve('keepMine')">保留我的版本并覆盖</button>
      <button type="button" @click="resolve('viewDiff')">查看差异</button>
      <button type="button" @click="store.closeNote(props.relPath)">关闭标签</button>
    </div>
    <div v-if="props.diff" class="kp-conflict__diff" data-testid="conflict-diff">
      <p class="kp-conflict__hint">差异（外部版本 → 我的版本）：</p>
      <pre
        v-for="(line, index) in props.diff"
        :key="index"
        class="kp-conflict__line"
        :class="lineClass(line.type)"
        data-testid="conflict-line"
      >{{ prefixOf(line.type) }}{{ line.text }}</pre>
    </div>
  </div>
</template>

<style scoped>
.kp-conflict { padding: 6px 8px; background: #fff3f3; border-bottom: 1px solid #f0c0c0; font-size: 12px; }
.kp-conflict__title { margin: 0 0 4px; }
.kp-conflict__actions { display: flex; gap: 6px; }
.kp-conflict__diff { max-height: 200px; overflow: auto; margin-top: 6px; background: #fff; }
.kp-conflict__line { margin: 0; white-space: pre-wrap; }
.kp-conflict__line--added { background: #e6ffed; }
.kp-conflict__line--removed { background: #ffeef0; }
.kp-conflict__line--same { color: #666; }
.kp-conflict__hint { margin: 4px 0; color: #666; }
</style>
