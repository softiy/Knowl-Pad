<script setup lang="ts">
import { ref } from 'vue';
import TagTreeLabel from './TagTreeLabel.vue';
import type { TagNode } from '@core/ipc/tagCommands';

/**
 * 标签树的一行 **+ 它的子孙**（FR-TAG-01/02）：递归组件，根是 `<li>`，
 * 子节点以组件身份渲染进同一个 `<ul>`（`<li>` 是组件根，DOM 结构因此仍然是合法的列表）。
 *
 * 折叠状态**由本行自持**：树里每行一个实例，父行不需要知道哪些子孙展开着，
 * 折叠也就不会引起父级重算（可见部分在 `TagTreeLabel`）。
 */
const props = defineProps<{ node: TagNode; depth: number; selectedNorm: string | null }>();

const emit = defineEmits<{ select: [norm: string] }>();

const collapsed = ref(false);

/** 折叠只影响**渲染**：不删数据、不改计数 —— 重新展开时计数照旧（它本来就来自索引库）。 */
function toggle(): void {
  collapsed.value = !collapsed.value;
}
</script>

<template>
  <li class="kp-tags__row" :data-depth="props.depth" :data-norm="props.node.norm" data-testid="tag-tree-row">
    <TagTreeLabel
      :node="props.node"
      :depth="props.depth"
      :selected-norm="props.selectedNorm"
      :collapsed="collapsed"
      @toggle="toggle"
      @select="(norm) => emit('select', norm)"
    />
  </li>
  <template v-if="!collapsed">
    <TagTreeRow
      v-for="child in props.node.children"
      :key="child.norm"
      :node="child"
      :depth="props.depth + 1"
      :selected-norm="props.selectedNorm"
      @select="(norm) => emit('select', norm)"
    />
  </template>
</template>

<style scoped>
.kp-tags__row { display: flex; align-items: center; gap: 2px; font-size: 13px; }
</style>
