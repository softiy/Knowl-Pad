<script setup lang="ts">
import { computed, ref } from 'vue';
import { useEditorStore } from '../stores/editor';

/** 多标签栏（FR-EDITOR-35）：打开/切换/关闭、**拖拽排序**、中键关闭、右键菜单、未保存标记与关闭确认。 */
const store = useEditorStore();
/** 待确认关闭的标签集合（FR-EDITOR-32：关闭未保存内容前必须提示）。 */
const pendingClose = ref<string[] | null>(null);
const dragIndex = ref<number | null>(null);
const menu = ref<{ x: number; y: number; relPath: string } | null>(null);
/** 保存失败而未能关闭的标签（R-07：留在界面上，由编辑器面板展示错误） */
const saveFailed = ref<string[]>([]);

const pendingNames = computed(() => (pendingClose.value ?? []).map((p) => baseName(p)).join('、'));

function baseName(relPath: string): string {
  return relPath.split('/').pop() ?? relPath;
}

/** 需要确认时返回 true（未保存内容一律先问，R-07 不静默丢弃）。 */
function confirmIfDirty(targets: string[], apply: () => void): void {
  const dirty = store.dirtyAmong(targets);
  if (dirty.length > 0) {
    pendingClose.value = targets;
    return;
  }
  apply();
}

function requestClose(relPath: string): void {
  confirmIfDirty([relPath], () => store.closeNote(relPath));
}

function confirmClose(mode: 'save' | 'discard'): void {
  const targets = pendingClose.value ?? [];
  pendingClose.value = null;
  if (targets.length === 0) return;
  if (mode === 'discard') {
    applyClose(targets);
    return;
  }
  void Promise.all(targets.filter((p) => store.isDirty(p)).map((p) => store.save(p))).then(() => {
    // R-07：**保存失败的标签必须保持打开**——save() 内部吞掉错误（写冲突/权限）后 promise 仍会 resolve，
    // 因此以「是否还脏」判定是否真的保存成功，绝不静默丢弃内容。
    const closable = targets.filter((p) => !store.isDirty(p));
    const blocked = targets.filter((p) => store.isDirty(p));
    if (blocked.length > 0) saveFailed.value = blocked;
    if (closable.length > 0) applyClose(closable);
  });
}

function applyClose(targets: string[]): void {
  if (targets.length === 1) {
    store.closeNote(targets[0]);
    return;
  }
  for (const relPath of targets) store.closeNote(relPath);
}

function onAuxClick(event: MouseEvent, relPath: string): void {
  if (event.button === 1) requestClose(relPath); // 中键关闭
}

function onContextMenu(event: MouseEvent, relPath: string): void {
  menu.value = { x: event.clientX, y: event.clientY, relPath };
}

function runMenuAction(action: 'close' | 'closeOthers' | 'closeRight' | 'copyPath'): void {
  const target = menu.value?.relPath;
  menu.value = null;
  if (!target) return;
  if (action === 'copyPath') {
    void navigator.clipboard?.writeText(target);
    return;
  }
  if (action === 'close') {
    requestClose(target);
    return;
  }
  if (action === 'closeOthers') {
    const others = store.buffers.map((b) => b.relPath).filter((p) => p !== target);
    confirmIfDirty(others, () => store.closeOthers(target));
    return;
  }
  const index = store.buffers.findIndex((b) => b.relPath === target);
  const right = store.buffers.slice(index + 1).map((b) => b.relPath);
  confirmIfDirty(right, () => store.closeToTheRight(target));
}

function onDragStart(index: number): void {
  dragIndex.value = index;
}

function onDrop(index: number): void {
  if (dragIndex.value !== null) {
    store.reorderTab(dragIndex.value, index);
  }
  dragIndex.value = null;
}
</script>

<template>
  <div class="kp-tabs">
    <div class="kp-tabs__list" role="tablist">
      <button
        v-for="(buffer, index) in store.buffers"
        :key="buffer.relPath"
        type="button"
        role="tab"
        draggable="true"
        class="kp-tabs__tab"
        :class="{ 'kp-tabs__tab--active': buffer.relPath === store.activeRelPath }"
        :data-testid="`tab-${buffer.relPath}`"
        :aria-selected="buffer.relPath === store.activeRelPath"
        @click="store.activate(buffer.relPath)"
        @auxclick="onAuxClick($event, buffer.relPath)"
        @contextmenu.prevent="onContextMenu($event, buffer.relPath)"
        @dragstart="onDragStart(index)"
        @dragover.prevent
        @drop.prevent="onDrop(index)"
      >
        <span class="kp-tabs__name">{{ baseName(buffer.relPath) }}</span>
        <span v-if="store.isDirty(buffer.relPath)" class="kp-tabs__dirty" data-testid="dirty-dot">●</span>
        <span class="kp-tabs__close" data-testid="tab-close" @click.stop="requestClose(buffer.relPath)">×</span>
      </button>
    </div>

    <div v-if="menu" class="kp-tabs__menu" data-testid="tab-menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }">
      <button type="button" @click="runMenuAction('close')">关闭</button>
      <button type="button" @click="runMenuAction('closeOthers')">关闭其他</button>
      <button type="button" @click="runMenuAction('closeRight')">关闭右侧</button>
      <button type="button" @click="runMenuAction('copyPath')">复制相对路径</button>
    </div>

    <div v-if="saveFailed.length > 0" class="kp-tabs__failed" data-testid="close-save-failed">
      <span>「{{ saveFailed.join('、') }}」保存失败，标签已保留（请处理后重试）</span>
      <button type="button" @click="saveFailed = []">知道了</button>
    </div>

    <div v-if="pendingClose" class="kp-tabs__confirm" data-testid="close-confirm">
      <span>「{{ pendingNames }}」有未保存的修改：</span>
      <button type="button" @click="confirmClose('save')">保存并关闭</button>
      <button type="button" @click="confirmClose('discard')">放弃修改</button>
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
.kp-tabs__menu { position: fixed; z-index: 100; display: flex; flex-direction: column; min-width: 140px; padding: 4px; background: #fff; border: 1px solid #ccc; border-radius: 4px; box-shadow: 0 4px 12px rgb(0 0 0 / 15%); }
.kp-tabs__menu button { text-align: left; padding: 4px 8px; background: none; border: 0; cursor: pointer; font-size: 13px; }
.kp-tabs__menu button:hover { background: #f2f2f2; }
.kp-tabs__failed { display: flex; gap: 8px; align-items: center; padding: 4px 8px; background: #ffeaea; color: #b00020; font-size: 12px; }
.kp-tabs__confirm { display: flex; gap: 8px; align-items: center; padding: 4px 8px; background: #fff8e1; font-size: 12px; }
</style>
