<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useVirtualizer } from '@tanstack/vue-virtual';
import { fileReveal } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import { useFileTreeStore } from '../stores/fileTree';
import { TREE_OVERSCAN, TREE_ROW_HEIGHT, type MenuAction, type TreeRow } from '../types';
import TreeContextMenu from './TreeContextMenu.vue';

const emit = defineEmits<{
  /** 打开笔记（双击或点击文件行） */
  open: [relPath: string];
  /** 需要对话框的动作交给上层（新建/重命名/删除） */
  action: [payload: { type: MenuAction; relPath: string; isDir: boolean }];
}>();

const store = useFileTreeStore();
const scrollRef = ref<HTMLElement | null>(null);
const menu = ref<{ visible: boolean; x: number; y: number; row: TreeRow | null }>({
  visible: false,
  x: 0,
  y: 0,
  row: null,
});

/** 虚拟滚动：行高固定 28px、上下各 10 行缓冲（技术方案 §7.3）。 */
const virtualizer = useVirtualizer(
  computed(() => ({
    count: store.rows.length,
    getScrollElement: () => scrollRef.value,
    estimateSize: () => TREE_ROW_HEIGHT,
    overscan: TREE_OVERSCAN,
  })),
);
const virtualRows = computed(() => virtualizer.value.getVirtualItems());

onMounted(() => {
  void store.restoreState();
});

function onRowClick(row: TreeRow): void {
  if (row.isDir) {
    void store.toggle(row.relPath);
    return;
  }
  emit('open', row.relPath);
}

function onRowContextMenu(event: MouseEvent, row: TreeRow): void {
  menu.value = { visible: true, x: event.clientX, y: event.clientY, row };
}

async function onMenuSelect(action: MenuAction): Promise<void> {
  const row = menu.value.row;
  menu.value.visible = false;
  if (!row) {
    return;
  }
  if (action === 'reveal') {
    try {
      await fileReveal(row.relPath);
    } catch (err) {
      store.error = asKpError(err).message;
    }
    return;
  }
  if (action === 'copyPath') {
    await navigator.clipboard?.writeText(row.relPath);
    return;
  }
  emit('action', { type: action, relPath: row.relPath, isDir: row.isDir });
}

/** 图标按类型区分（FR-FILE-03）；用文本符号避免引入图标库。 */
function iconOf(row: TreeRow): string {
  if (row.isDir) {
    return row.expanded ? '📂' : '📁';
  }
  if (row.kind === 'attachment') {
    return '📎';
  }
  return row.kind === 'note' ? '📝' : '📄';
}
</script>

<template>
  <div class="file-tree">
    <div class="file-tree__toolbar">
      <label>
        <input
          type="checkbox"
          :checked="store.includeHidden"
          @change="store.setIncludeHidden(($event.target as HTMLInputElement).checked)"
        >
        显示隐藏文件
      </label>
    </div>
    <p v-if="store.error" class="file-tree__error">{{ store.error }}</p>
    <div ref="scrollRef" class="file-tree__scroll" data-testid="tree-scroll">
      <div class="file-tree__spacer" :style="{ height: `${virtualizer.getTotalSize()}px` }">
        <div
          v-for="item in virtualRows"
          :key="item.index"
          class="file-tree__row"
          data-testid="tree-row"
          :style="{ transform: `translateY(${item.start}px)`, height: `${TREE_ROW_HEIGHT}px` }"
          @click="onRowClick(store.rows[item.index])"
          @contextmenu.prevent="onRowContextMenu($event, store.rows[item.index])"
        >
          <span
            class="file-tree__indent"
            :style="{ width: `${store.rows[item.index].depth * 12}px` }"
          />
          <span class="file-tree__icon">{{ iconOf(store.rows[item.index]) }}</span>
          <span class="file-tree__name">{{ store.rows[item.index].name }}</span>
        </div>
      </div>
    </div>
    <TreeContextMenu
      v-if="menu.visible && menu.row"
      :x="menu.x"
      :y="menu.y"
      :is-dir="menu.row.isDir"
      :name="menu.row.name"
      @select="onMenuSelect"
      @close="menu.visible = false"
    />
  </div>
</template>

<style scoped>
.file-tree { display: flex; flex-direction: column; height: 100%; font-size: 13px; }
.file-tree__toolbar { padding: 4px 8px; border-bottom: 1px solid var(--kp-border, #ddd); }
.file-tree__error { margin: 0; padding: 4px 8px; color: #b00020; }
.file-tree__scroll { flex: 1; overflow: auto; position: relative; }
.file-tree__spacer { position: relative; width: 100%; }
.file-tree__row { position: absolute; top: 0; left: 0; right: 0; display: flex; align-items: center; gap: 4px; padding: 0 8px; cursor: default; white-space: nowrap; }
.file-tree__row:hover { background: var(--kp-hover, #f2f2f2); }
.file-tree__indent { flex: none; }
.file-tree__name { overflow: hidden; text-overflow: ellipsis; }
</style>
