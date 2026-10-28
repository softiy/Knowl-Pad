<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { createEditorAdapter, type KpEditorAdapter } from '../adapter';
import { consumeReveal } from '../adapter/consumeReveal';
import { createClickRouting } from '../composables/useClickRouting';
import { AUTOSAVE_DEFAULT_MS, useEditorStore, type ConflictChoice, type EditorMode } from '../stores/editor';
import ConflictDialog from './ConflictDialog.vue';
import FindReplacePanel from './FindReplacePanel.vue';
import { useFindPanel } from '../composables/useFindPanel';
import { useEditorBufferSync } from '../composables/useEditorBufferSync';
import { bindEditorShortcuts } from '../composables/useEditorShortcuts';
import EditorTabs from './EditorTabs.vue';
import MarkdownView from './MarkdownView.vue';

/**
 * 编辑器主视图：
 * - 三态（纯编辑 / 分屏 / 纯阅读，FR-EDITOR-04）；阅读与分屏的预览走 §7.2 净化管线；
 * - 内核交互只经 `KpEditorAdapter`（§7.4），换内核（M8）不动本组件；
 * - 快捷键统一走 `core/shortcut`（ED-04），不在这里裸绑 keydown 语义；
 * - `destroy()`/注销函数在卸载时全部调用（ED-01）。
 */
/** 阅读态的跳转出口（可选注入；缺省只做编辑器自己会做的事，见 composables/useClickRouting.ts）。 */
const props = defineProps<{
  openLink?: ((target: string, event?: MouseEvent) => void) | undefined;
  openTag?: ((tag: string) => void) | undefined;
  createNote?: ((name: string) => void) | undefined;
}>();

const store = useEditorStore();
const hostRef = ref<HTMLElement | null>(null);
let adapter: KpEditorAdapter | null = null;
let disposeShortcuts: (() => void) | null = null;

const active = computed(() => store.buffers.find((b) => b.relPath === store.activeRelPath) ?? null);
const modes: { value: EditorMode; label: string }[] = [
  { value: 'edit', label: '编辑' },
  { value: 'split', label: '分屏' },
  { value: 'read', label: '阅读' },
];
const autosaveOptions = [500, AUTOSAVE_DEFAULT_MS, 2000, 3000, 5000];

const { findOpen, findMode, findPanel, openFind, toggleFind, closeFind } = useFindPanel(hostRef);
void findPanel; // 模板 ref 由组件绑定，脚本侧仅需保留引用（类型检查用）
consumeReveal({ current: () => store.pendingReveal, active: () => store.activeRelPath, clear: () => store.clearReveal(), adapter: () => adapter });

onMounted(() => {
  // 恢复持久化的自动保存防抖时长（DEBT-30）：此前只在内存里，重启会回到默认值。
  void store.loadAutosaveDelay();
  adapter = createEditorAdapter();
  if (hostRef.value) {
    adapter.mount(hostRef.value, { initialValue: active.value?.content ?? '' });
  }
  adapter.on('change', (value) => {
    if (store.activeRelPath) store.updateContent(store.activeRelPath, String(value));
  });
  adapter.on('save', () => {
    if (store.activeRelPath) void store.save(store.activeRelPath);
  });
  disposeShortcuts = bindEditorShortcuts({
    save: () => {
      if (store.activeRelPath) void store.save(store.activeRelPath);
    },
    openFind,
  });
});

// 切换标签 / 「加载外部版本」时把缓冲区灌进内核（细节与 AC-EDITOR-06 的边界见组合式）。
useEditorBufferSync({
  relPath: () => store.activeRelPath,
  replacedAt: () => active.value?.replacedAt,
  content: () => active.value?.content ?? '',
  adapter: () => adapter,
});

onBeforeUnmount(() => {
  disposeShortcuts?.();
  disposeShortcuts = null;
  adapter?.destroy();
  adapter = null;
});

function resolveConflict(choice: ConflictChoice): void {
  if (store.activeRelPath) void store.resolveConflict(store.activeRelPath, choice);
}

const emit = defineEmits<{
  /** 已判定悬空的链接被"普通点击" ⇒ 交给上层走创建流程（FR-EDITOR-27） */
  create: [name: string];
  /** 标签被点击 ⇒ 交给上层打开标签面板并选中（FR-TAG-05） */
  tag: [tag: string];
}>();

// 阅读态点击的**分派**：判定在 logic/linkRouting.ts，接线在 composables/useClickRouting.ts
const routing = createClickRouting({
  fallbackOpen: () => (target: string) => void store.reveal(target, 1),
  openLink: () => props.openLink,
  createNote: () => props.createNote,
  openTag: () => props.openTag,
  emitCreate: (name) => emit('create', name),
  emitTag: (tag) => emit('tag', tag),
});
</script>

<template>
  <section class="kp-editor">
    <header class="kp-editor__toolbar">
      <div class="kp-editor__modes" role="group" aria-label="视图模式">
        <button
          v-for="item in modes"
          :key="item.value"
          type="button"
          :class="{ 'kp-editor__mode--active': store.mode === item.value }"
          :data-testid="`mode-${item.value}`"
          @click="store.setMode(item.value)"
        >
          {{ item.label }}
        </button>
      </div>
      <label class="kp-editor__autosave">
        自动保存
        <select
          :value="store.autosaveDelayMs"
          data-testid="autosave-delay"
          @change="store.setAutosaveDelay(Number(($event.target as HTMLSelectElement).value))"
        >
          <option v-for="ms in autosaveOptions" :key="ms" :value="ms">{{ ms / 1000 }} 秒</option>
        </select>
      </label>
      <button type="button" data-testid="find-toggle" @click="toggleFind">查找替换</button>
      <span v-if="active?.saving" class="kp-editor__hint" data-testid="saving">保存中…</span>
      <span v-else-if="active && store.isDirty(active.relPath)" class="kp-editor__hint">未保存</span>
    </header>

    <EditorTabs />

    <FindReplacePanel
      v-if="findOpen" ref="findPanel"
      :adapter="adapter"
      :content="active?.content ?? ''"
      :mode="findMode"
      @close="closeFind"
    />

    <ConflictDialog
      v-if="active?.conflict"
      :rel-path="active.relPath"
      :diff="active.diff"
      @resolve="resolveConflict"
    />

    <p v-if="store.openError" class="kp-editor__error" data-testid="open-error">
      {{ store.openError }}
    </p>
    <p v-if="active?.error && !active.conflict" class="kp-editor__error" data-testid="editor-error">
      {{ active.error }}
    </p>

    <p v-if="!active" class="kp-editor__empty" data-testid="editor-empty">从文件树打开一篇笔记开始编辑</p>

    <div class="kp-editor__body">
      <div v-show="store.mode !== 'read'" ref="hostRef" class="kp-editor__host" data-testid="editor-host" />
      <div v-show="store.mode !== 'edit'" class="kp-editor__preview" data-testid="editor-preview">
        <MarkdownView
          v-if="active"
          :source="active.content ?? ''"
          :rel-path="active.relPath"
          :cache-scope="active.relPath"
          @link="routing.onLink"
          @tag="routing.onTag"
        />
      </div>
    </div>
  </section>
</template>

<style scoped>
.kp-editor { display: flex; flex-direction: column; height: 100%; }
.kp-editor__toolbar { display: flex; align-items: center; gap: 12px; padding: 4px 8px; border-bottom: 1px solid var(--kp-border, #ddd); font-size: 12px; }
.kp-editor__modes button, .kp-editor__autosave select { font-size: 12px; }
.kp-editor__mode--active { font-weight: 600; }
.kp-editor__hint { color: #b26a00; }
.kp-editor__error { margin: 0; padding: 6px 8px; color: #b00020; font-size: 12px; }
.kp-editor__empty { margin: 0; padding: 12px; color: #666; }
.kp-editor__body { flex: 1; display: flex; min-height: 0; }
.kp-editor__host { flex: 1; min-width: 0; overflow: auto; }
.kp-editor__preview { flex: 1; min-width: 0; overflow: auto; padding: 8px 12px; border-left: 1px solid var(--kp-border, #ddd); }
</style>
