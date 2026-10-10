<script setup lang="ts">
import { computed, ref } from 'vue';
import type { BacklinkGroup } from '@core/ipc/linkCommands';

/**
 * 反链面板（FR-LINK-10~18）。
 *
 * 分组与折叠（FR-LINK-14）、普通与嵌入**分开计数**（FR-LINK-15）、行号与上下文片段
 * （FR-LINK-11）、点击跳转（FR-LINK-13）。
 *
 * FR-LINK-18（>500 条用虚拟滚动）：这里对**分组**做定高窗口化 —— 只渲染视口内的若干组，
 * 用一个撑高的占位元素保持滚动条比例。定高是刻意的简化：分组头高度固定，正文在展开时
 * 由"展开时才渲染"这一条约束保证不会一次性铺开上千行。
 */
const props = defineProps<{ groups: BacklinkGroup[] }>();
const emit = defineEmits<{ (e: 'open', payload: { relPath: string; line: number }): void }>();

/** 单个分组头的估算高度（含上下留白），窗口化按它换算。 */
const GROUP_HEIGHT = 28;
/** 视口上下各多渲染一组，减少快速滚动时的空白。 */
const OVERSCAN = 4;
/** 低于这个数量就整列渲染：窗口化对少量数据只会添乱。 */
const VIRTUAL_THRESHOLD = 100;

const scrollerRef = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(400);
const collapsed = ref<Record<string, boolean>>({});

const totalLinks = computed(() => props.groups.reduce((n, g) => n + g.linkCount, 0));
const totalEmbeds = computed(() => props.groups.reduce((n, g) => n + g.embedCount, 0));
const virtual = computed(() => props.groups.length > VIRTUAL_THRESHOLD);

const windowStart = computed(() =>
  virtual.value ? Math.max(0, Math.floor(scrollTop.value / GROUP_HEIGHT) - OVERSCAN) : 0,
);
const windowEnd = computed(() => {
  if (!virtual.value) return props.groups.length;
  const visible = Math.ceil(viewportHeight.value / GROUP_HEIGHT) + OVERSCAN * 2;
  return Math.min(props.groups.length, windowStart.value + visible);
});
const visibleGroups = computed(() => props.groups.slice(windowStart.value, windowEnd.value));
const offsetY = computed(() => windowStart.value * GROUP_HEIGHT);
const spacerHeight = computed(() => (virtual.value ? props.groups.length * GROUP_HEIGHT : 0));

function onScroll(): void {
  const el = scrollerRef.value;
  if (!el) return;
  scrollTop.value = el.scrollTop;
  viewportHeight.value = el.clientHeight || viewportHeight.value;
}

function toggle(relPath: string): void {
  collapsed.value = { ...collapsed.value, [relPath]: !collapsed.value[relPath] };
}
</script>

<template>
  <div
    ref="scrollerRef"
    class="kp-backlinks"
    data-testid="backlink-panel"
    @scroll="onScroll"
  >
    <p class="kp-backlinks__summary" data-testid="backlink-summary">
      反链 {{ totalLinks }} 条<template v-if="totalEmbeds > 0">，另有嵌入 {{ totalEmbeds }} 处</template>
    </p>
    <p v-if="groups.length === 0" class="kp-backlinks__empty" data-testid="backlink-empty">
      没有指向这篇笔记的链接
    </p>

    <div class="kp-backlinks__spacer" :style="{ height: spacerHeight + 'px' }">
      <div class="kp-backlinks__window" :style="{ transform: 'translateY(' + offsetY + 'px)' }">
        <section
          v-for="g in visibleGroups"
          :key="g.srcRelPath"
          class="kp-backlinks__group"
          data-testid="backlink-group"
        >
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
      </div>
    </div>

    <p v-if="virtual" class="kp-backlinks__hint" data-testid="backlink-virtual-hint">
      共 {{ groups.length }} 个来源，已启用虚拟滚动（当前渲染 {{ visibleGroups.length }} 个）
    </p>
  </div>
</template>

<style scoped>
.kp-backlinks { max-height: 100%; overflow: auto; }
.kp-backlinks__summary { color: #666; margin: 0 0 8px; }
.kp-backlinks__empty { color: #888; }
.kp-backlinks__hint { color: #888; font-size: 12px; }
.kp-backlinks__group { margin-bottom: 10px; }
.kp-backlinks__head { display: flex; align-items: center; gap: 8px; }
.kp-backlinks__counts { color: #666; font-size: 12px; }
.kp-backlinks__path { color: #999; font-size: 12px; margin-left: auto; }
.kp-backlinks__items { list-style: none; margin: 4px 0 0; padding-left: 16px; }
.kp-backlinks__snippet { background: #f6f6f6; font-size: 12px; margin: 2px 0 6px; padding: 4px 6px; white-space: pre-wrap; }
.kp-backlinks__embed, .kp-backlinks__alias { color: #b26a00; font-size: 12px; margin-left: 6px; }
</style>
