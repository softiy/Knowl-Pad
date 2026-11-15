<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useVirtualizer } from '@tanstack/vue-virtual';
import { fileReveal } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import { useFileTreeStore } from '../stores/fileTree';
import { useFileOpsStore } from '../stores/fileOps';
import { useFileTreeFsSync } from '../composables/useFileTreeFsSync';
import { wikilinkOf } from '../logic';
import {
  SORT_OPTIONS,
  TREE_OVERSCAN,
  TREE_ROW_HEIGHT,
  type MenuAction,
  type TreeRow,
  type TreeSortMode,
} from '../types';
import TreeContextMenu from './TreeContextMenu.vue';

const emit = defineEmits<{
  /** 打开笔记（双击或点击文件行） */
  open: [relPath: string];
  /** 需要对话框的动作交给上层（新建/重命名/删除） */
  action: [payload: { type: MenuAction; relPath: string; isDir: boolean }];
}>();

const store = useFileTreeStore();
/** 文件操作提示（M5 冒烟 5.5：复制 Wikilink / 路径后必须有可见反馈，且会自动消失） */
const ops = useFileOpsStore();
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

/** 订阅 kp://fs/*（规则与生命周期见 composable；让排序与树反映运行期外部改动）。
 * 放在 restoreState 的 onMounted 之后注册：先恢复树、再挂监听（与拆分前同一执行顺序）。 */
useFileTreeFsSync(store);

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

/** 剪贴板写入的统一出口：成功/失败都给出可见反馈（R-15：不静默，也不假装成功）。 */
async function copyText(text: string, label: string): Promise<void> {
  try {
    if (!navigator.clipboard) throw new Error('当前环境不支持剪贴板');
    await navigator.clipboard.writeText(text);
    ops.setNotice(`已复制 ${label}：${text}`);
  } catch (err) {
    store.error = asKpError(err).message;
    ops.setNotice(`复制 ${label} 失败`, 'error');
  }
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
    await copyText(row.relPath, '相对路径');
    return;
  }
  if (action === 'copyWikilink') {
    // FR-FILE-07：复制的是 `[[目标]]` 文本（笔记去 .md、附件保留扩展名），不是路径本身
    await copyText(wikilinkOf(row.relPath), 'Wikilink');
    return;
  }
  emit('action', { type: action, relPath: row.relPath, isDir: row.isDir });
}

/** FR-FILE-02：排序切换（纯前端重排，见 store 的 `setSortMode`）。 */
function onSortChange(event: Event): void {
  store.setSortMode((event.target as HTMLSelectElement).value as TreeSortMode);
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
    <!-- M5 冒烟：头部必须**单行**放下隐藏文件开关 + 排序 + 方向（不要换行挤成两行） -->
    <div class="file-tree__toolbar">
      <label class="file-tree__hidden">
        <input
          type="checkbox"
          :checked="store.includeHidden"
          @change="store.setIncludeHidden(($event.target as HTMLInputElement).checked)"
        >
        显示隐藏文件
      </label>
      <label class="file-tree__sort">
        排序
        <select data-testid="tree-sort" :value="store.sortMode" @change="onSortChange">
          <option v-for="opt in SORT_OPTIONS" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
      </label>
      <button
        type="button"
        class="file-tree__dir"
        data-testid="tree-sort-dir"
        :title="store.sortDirection === 'asc' ? '当前升序，点击切换为降序' : '当前降序，点击切换为升序'"
        :aria-label="store.sortDirection === 'asc' ? '升序，点击切换为降序' : '降序，点击切换为升序'"
        @click="store.toggleSortDirection()"
      >
        {{ store.sortDirection === 'asc' ? '↑' : '↓' }}
      </button>
    </div>
    <p v-if="store.error" class="file-tree__error">{{ store.error }}</p>
    <div ref="scrollRef" class="file-tree__scroll" data-testid="tree-scroll">
      <div class="file-tree__spacer" :style="{ height: `${virtualizer.getTotalSize()}px` }">
        <div
          v-for="item in virtualRows"
          :key="item.index"
          class="file-tree__row"
          data-testid="tree-row"
          :title="store.rows[item.index].name"
          :style="{ transform: `translateY(${item.start}px)`, height: `${TREE_ROW_HEIGHT}px` }"
          @click="onRowClick(store.rows[item.index])"
          @contextmenu.prevent="onRowContextMenu($event, store.rows[item.index])"
        >
          <span
            class="file-tree__indent"
            :style="{ width: `${store.rows[item.index].depth * 10}px` }"
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
.file-tree { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow: hidden; font-size: 13px; }
/* 单行 + 紧凑：窄侧栏（layout 侧最小 240px）内也不换行；极端窄时横向滚动而不是截断 */
.file-tree__toolbar { display: flex; flex-wrap: nowrap; align-items: center; gap: 4px; padding: 2px 5px; overflow-x: auto; white-space: nowrap; font-size: 12px; border-bottom: 1px solid var(--kp-border, #ddd); }
.file-tree__toolbar label { display: flex; align-items: center; gap: 3px; flex: 0 0 auto; }
.file-tree__toolbar input[type='checkbox'] { margin: 0; }
.file-tree__sort { margin-left: auto; }
.file-tree__sort select { max-width: 72px; font-size: 12px; }
.file-tree__dir { flex: 0 0 auto; min-width: 22px; padding: 0 4px; background: none; border: 1px solid var(--kp-border, #ddd); border-radius: 3px; cursor: pointer; font-size: 12px; line-height: 16px; }
.file-tree__dir:hover { background: var(--kp-hover, #f2f2f2); }
.file-tree__error { margin: 0; padding: 4px 8px; color: #b00020; }
/* 树容器：flex 子项必须 min-height:0 才会真正产生滚动条（否则高度被内容撑开 / 溢出被裁掉） */
.file-tree__scroll { flex: 1 1 auto; min-height: 0; overflow: auto; position: relative; }
.file-tree__spacer { position: relative; width: 100%; }
.file-tree__row { position: absolute; top: 0; left: 0; right: 0; display: flex; align-items: center; gap: 4px; padding: 0 6px; cursor: default; white-space: nowrap; }
.file-tree__row:hover { background: var(--kp-hover, #f2f2f2); }
.file-tree__indent { flex: none; }
.file-tree__name { overflow: hidden; text-overflow: ellipsis; }
</style>