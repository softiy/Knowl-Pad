<script setup lang="ts">
import { isSelectedNode, refCountLabel } from '../logic';
import type { TagNode } from '@core/ipc/tagCommands';

/**
 * 标签树**一行**的展示（FR-TAG-01/02/13 的可见部分）：缩进、折叠开关、名字、计数。
 *
 * 每条口径都不是随手定的：
 * ① 缩进按**层级**（`a/b/c` 显示为 a → b → c 三级嵌套，FR-TAG-02）；
 * ② 每级显示**自己的 refCount**（file_tag **行数**口径），**不显示"子孙合计"**：
 *    祖先登记让祖先的计数天然已含子孙（PRD §5.3.6.1），再加一次就是重复计数；
 * ③ 每级节点都显示，**不只显示叶子** —— 分支自身的 `isLeaf` 为假，用徽标标出，
 *    这样"点击标签看它的笔记"对分支也成立（FR-TAG-03）；
 * ④ 「未分类」的计数单位是**篇数**（它没有 file_tag 行），文案由 `refCountLabel` 区分。
 */
const props = defineProps<{
  node: TagNode;
  depth: number;
  selectedNorm: string | null;
  collapsed: boolean;
}>();

const emit = defineEmits<{ toggle: []; select: [norm: string] }>();

const hasChildren = (): boolean => props.node.children.length > 0;
/** 折叠图标；叶子节点没有可折叠的内容，给一个不可点的中点（而不是假装能展开）。 */
const toggleGlyph = (): string => (hasChildren() ? (props.collapsed ? '▶' : '▼') : '·');
</script>

<template>
  <span class="kp-tags__indent" :style="{ paddingLeft: (props.depth - 1) * 12 + 'px' }" />
  <button
    type="button"
    class="kp-tags__toggle"
    :disabled="!hasChildren()"
    data-testid="tag-toggle"
    :data-norm="props.node.norm"
    :aria-expanded="hasChildren() && !props.collapsed"
    @click="emit('toggle')"
  >
    {{ toggleGlyph() }}
  </button>
  <button
    type="button"
    class="kp-tags__name"
    :class="{ 'kp-tags__name--active': isSelectedNode(props.node, props.selectedNorm) }"
    data-testid="tag-node"
    :data-norm="props.node.norm"
    @click="emit('select', props.node.norm)"
  >
    {{ props.node.display }}
  </button>
  <span v-if="props.node.unclassified" class="kp-tags__badge" data-testid="tag-unclassified-badge">未分类</span>
  <span v-else-if="!props.node.isLeaf" class="kp-tags__badge" data-testid="tag-branch-badge">分支</span>
  <span class="kp-tags__count" data-testid="tag-count">{{ refCountLabel(props.node) }}</span>
</template>

<style scoped>
.kp-tags__toggle { width: 18px; padding: 0; background: none; border: 0; cursor: pointer; color: #666; }
.kp-tags__toggle:disabled { cursor: default; color: #ccc; }
.kp-tags__name { flex: 1; min-width: 0; text-align: left; background: none; border: 0; cursor: pointer; padding: 2px 4px; }
.kp-tags__name--active { background: #e3f2fd; font-weight: 600; }
.kp-tags__badge { font-size: 10px; color: #888; border: 1px solid #ddd; border-radius: 3px; padding: 0 3px; }
.kp-tags__count { font-size: 11px; color: #666; }
</style>
