<script setup lang="ts">
import TagTreeRow from './TagTreeRow.vue';
import type { TagNode } from '@core/ipc/tagCommands';

/**
 * 标签树容器（FR-TAG-01/02/13）。
 *
 * 只做两件事：把根节点交给递归行组件、把点击**向上转发**。
 * 折叠状态由每行自持（见 `TagTreeRow`），容器不参与 —— 于是"折叠"不会引起父级重算。
 */
const props = defineProps<{ nodes: TagNode[]; selectedNorm: string | null }>();

const emit = defineEmits<{ select: [norm: string] }>();
</script>

<template>
  <ul class="kp-tags__tree" data-testid="tag-tree">
    <TagTreeRow
      v-for="node in props.nodes"
      :key="node.norm === '' ? 'unclassified' : node.norm"
      :node="node"
      :depth="1"
      :selected-norm="props.selectedNorm"
      @select="(norm) => emit('select', norm)"
    />
  </ul>
</template>

<style scoped>
.kp-tags__tree { list-style: none; margin: 0; padding: 0; }
</style>
