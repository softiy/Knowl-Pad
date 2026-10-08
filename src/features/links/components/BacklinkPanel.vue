<script setup lang="ts">
import { computed, ref } from 'vue';
import type { BacklinkGroup } from '@core/ipc/linkCommands';

/**
 * 反链面板（FR-LINK-10~18）。
 *
 * 分组与折叠（FR-LINK-14）、普通与嵌入**分开计数**（FR-LINK-15）、
 * 行号与上下文片段（FR-LINK-11）、点击跳转（FR-LINK-13 的第一半）。
 *
 * FR-LINK-18（>500 条虚拟滚动）：本版先做"默认只渲染前 N 组 + 可展开"，
 * 真正的虚拟滚动登记顺延（见 M4 计划的遗留项）。
 */
const props = defineProps<{ groups: BacklinkGroup[] }>();
const emit = defineEmits<{ (e: 'open', payload: { relPath: string; line: number }): void }>();

/** 默认渲染上限：超过它先折叠，避免一次性渲染上千条。 */
const DEFAULT_VISIBLE = 50;
const expanded = ref(false);
const collapsed = ref<Record<string, boolean>>({});

const totalLinks = computed(() => props.groups.reduce((n, g) => n + g.linkCount, 0));
const totalEmbeds = computed(() => props.groups.reduce((n, g) => n + g.embedCount, 0));
const visible = computed(() => (expanded.value ? props.groups : props.groups.slice(0, DEFAULT_VISIBLE)));

function toggle(relPath: string): void {
  collapsed.value = { ...collapsed.value, [relPath]: !collapsed.value[relPath] };
}
</script>

<template>
  <div class="kp-backlinks" data-testid="backlink-panel">
    <p class="kp-backlinks__summary" data-testid="backlink-summary">
      反链 {{ totalLinks }} 条<template v-if="totalEmbeds > 0">，另有嵌入 {{ totalEmbeds }} 处</template>
    </p>
    <p v-if="groups.length === 0" class="kp-backlinks__empty" data-testid="backlink-empty">
      没有指向这篇笔记的链接
    </p>

    <section v-for="g in visible" :key="g.srcRelPath" class="kp-backlinks__group">
      <header class="kp-backlinks__head">
        <button type="button" :data-testid="'backlink-toggle-' + g.srcName" @click="toggle(g.srcRelPath)">
          {{ collapsed[g.srcRelPath] ? '▸' : '▾' }} {{ g.srcName }}
        </button>
        <span class="kp-backlinks__counts" data-testid="backlink-counts">
          链接 {{ g.linkCount }} · 嵌入 {{ g.embedCount }}
        </span>
        <span class="kp-backlinks__path">{{ g.srcRelPath }}</span>
      </header>

      <ul v-if="!collapsed[g.srcRelPath]" class="kp-backlinks__items">
        <li v-for="(item, i) in g.items" :key="g.srcRelPath + '-' + i">
          <button
            type="button"
            data-testid="backlink-item"
            @click="emit('open', { relPath: g.srcRelPath, line: item.line })"
          >
            第 {{ item.line }} 行
            <span v-if="item.linkKind === 'embed'" class="kp-backlinks__embed">嵌入</span>
            <span v-if="item.alias" class="kp-backlinks__alias">别名 {{ item.alias }}</span>
          </button>
          <pre v-if="item.snippet" class="kp-backlinks__snippet" data-testid="backlink-snippet">{{ item.snippet.lines.map((l) => l.line + '  ' + l.text).join('\n') }}</pre>
        </li>
      </ul>
    </section>

    <button
      v-if="!expanded && groups.length > DEFAULT_VISIBLE"
      type="button"
      data-testid="backlink-show-more"
      @click="expanded = true"
    >
      还有 {{ groups.length - DEFAULT_VISIBLE }} 个来源（点击展开）
    </button>
  </div>
</template>

<style scoped>
.kp-backlinks__summary { color: #666; margin: 0 0 8px; }
.kp-backlinks__empty { color: #888; }
.kp-backlinks__group { margin-bottom: 10px; }
.kp-backlinks__head { display: flex; align-items: center; gap: 8px; }
.kp-backlinks__counts { color: #666; font-size: 12px; }
.kp-backlinks__path { color: #999; font-size: 12px; margin-left: auto; }
.kp-backlinks__items { list-style: none; margin: 4px 0 0; padding-left: 16px; }
.kp-backlinks__snippet { background: #f6f6f6; font-size: 12px; margin: 2px 0 6px; padding: 4px 6px; white-space: pre-wrap; }
.kp-backlinks__embed, .kp-backlinks__alias { color: #b26a00; font-size: 12px; margin-left: 6px; }
</style>
