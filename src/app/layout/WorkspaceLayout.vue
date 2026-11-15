<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { vaultStateGet, vaultStateSet } from '@core/ipc/commands';
import { assetUrlFor } from '@core/ipc/assetCommands';
import { bindShortcuts } from '@core/shortcut';
import { useVaultStore } from '@features/vault';
import { EditorView, useEditorStore } from '@features/editor';
import { FileOpsDialog, FileTree, useFileOpsStore, useFileTreeStore, type MenuAction } from '@features/file-tree';
import { LinksPanel } from '@features/links';
import { QuickOpen, SearchPanel, useSearchStore } from '@features/search';
import { TagPanel, useTagsStore } from '@features/tags';
import { CommandPalette, usePaletteStore } from '@features/palette';
import { SettingsPanel, useSettingsStore } from '@features/settings';
import {
  Ribbon, SIDEBAR_MIN_WIDTH, createPaletteActions, useLayoutStore, useWorkspaceStore,
} from '@features/layout';
import VaultList from '@features/vault/components/VaultList.vue';
import StatusBar from './StatusBar.vue';

/**
 * 工作区布局（技术方案 §2.2 的 `app/layout`）：Ribbon + 左文件树 + 中编辑器 + 右单一 dock + 状态栏。
 *
 * 本组件只做「渲染 + 组件 ref」；跨 feature 的时序编排（切库重置、右栏互斥、命令执行体、
 * 保存失败语义）在 `@features/layout` 的 workspace store 里，可被单测直接覆盖。
 */
const vaultStore = useVaultStore();
const editor = useEditorStore();
const search = useSearchStore();
const tags = useTagsStore();
const palette = usePaletteStore();
const fileTree = useFileTreeStore();
const ops = useFileOpsStore();
const settings = useSettingsStore();
const layout = useLayoutStore();
const workspace = useWorkspaceStore();
const searchPanel = ref<InstanceType<typeof SearchPanel> | null>(null);
const quickOpen = ref<InstanceType<typeof QuickOpen> | null>(null);
const tagPanel = ref<InstanceType<typeof TagPanel> | null>(null);
let unbind: (() => void) | null = null;
let untrackViewport: (() => void) | null = null;

// FR-LAYOUT-01：布局按 PRD 记在 vault_state（键 ui.sidebarCollapsed / ui.sidebarWidth / ui.rightPanel）。
layout.configureVaultState({
  read: async (keys) => (await vaultStateGet(keys)).values,
  write: async (key, value) => { await vaultStateSet({ [key]: value }); },
});
// AC-SET-02：Vault 级设置只认「当前 Vault」；read 里同时按新库恢复文件树展开态（AC-FILE-08）。
settings.configureVaultScope({
  read: async () => {
    await fileTree.restoreState();
    return fileTree.includeHidden;
  },
  write: async (value) => { await fileTree.setIncludeHidden(value); },
});
// S2：自动保存防抖的唯一来源是 editor store（设置面板与编辑器工具栏必须同源）。
settings.configureAutosave({
  get: () => editor.autosaveDelayMs,
  set: (ms) => editor.setAutosaveDelay(ms),
});

function focusSearch(): void { void nextTick(() => { searchPanel.value?.focusInput(); }); }
function focusTags(): void { void nextTick(() => { tagPanel.value?.focusFilter(); }); }
function focusQuickOpen(): void { void nextTick(() => { quickOpen.value?.focusInput(); }); }

// 命令面板（FR-PALETTE-01/03/07）：执行体在这一层注入（跨 feature 编排，FE-02）。
palette.configure(createPaletteActions(workspace, { quickOpen: focusQuickOpen, search: focusSearch, tags: focusTags }));

// 9.5：启动时路由守卫已 restore current，watch 默认不触发 —— onMounted 必须主动 apply 一次。
// 9.6：之后切库（含 current→null）统一走 applyVaultScope：先清跨库状态，再恢复新库的持久化布局。
watch(() => vaultStore.current?.vaultId ?? null, (vaultId) => { void workspace.applyVaultScope(vaultId); });

/** 打开命令面板：先刷新上下文（FR-PALETTE-01 的置灰判定按当前状态算）。 */
function openPalette(): void {
  palette.open({
    hasActiveNote: editor.activeRelPath !== null,
    hasVault: vaultStore.current !== null,
    mode: editor.mode === 'read' ? 'read' : 'edit',
    includeHidden: fileTree.includeHidden,
  });
}

onMounted(() => {
  void vaultStore.init();
  void workspace.checkGlobalRecovery();
  untrackViewport = workspace.trackViewport(); // FR-LAYOUT-04：只注册一次，卸载注销
  void workspace.applyVaultScope(vaultStore.current?.vaultId ?? null);
  // 快捷键统一走 core/shortcut（ED-04：组件不自行绑 keydown），卸载时必须注销（ED-01）。
  unbind = bindShortcuts(window, [
    { spec: 'Mod+Shift+F', handler: () => { workspace.openSearch(); focusSearch(); } },
    { spec: 'Mod+O', handler: () => { workspace.quickOpen(); focusQuickOpen(); } },
    { spec: 'Mod+P', handler: openPalette },
    // Mod+Shift+T：Mod+Shift+F 已给搜索、Mod+O 给快速打开，避开二者。
    { spec: 'Mod+Shift+T', handler: () => { workspace.openTags(); focusTags(); } },
  ]);
});

onUnmounted(() => {
  unbind?.();
  unbind = null;
  untrackViewport?.();
  untrackViewport = null;
});

function onOpen(relPath: string): void {
  void editor.openNote(relPath);
}

/** 点击反链/孤立项：打开来源笔记（FR-LINK-13 的第一半；滚动定位与高亮见遗留项）。 */
function onOpenLink(payload: { relPath: string; line: number }): void {
  void editor.reveal(payload.relPath, payload.line);
}

/** 点击搜索结果：带上命中列区间 ⇒ 精确选中命中处（FR-SEARCH-07）；无区间时回退整行。 */
function onOpenHit(relPath: string, line: number, range?: { start: number; end: number }): void {
  void editor.reveal(relPath, line, range);
}

/** 阅读态 wikilink 的跳转出口（FR-EDITOR-25）：已裁决 → 既有 reveal 通路；悬空 → 既有新建流程。 */
function onOpenWikiLink(target: string, event?: MouseEvent): void {
  // FR-EDITOR-25：Ctrl/Cmd+Alt+点击 ⇒ 新标签页打开（不切走当前标签）
  if (event?.altKey === true) void editor.openNoteInNewTab(target);
  else void editor.reveal(target, 1);
}

function onCreateFromLink(name: string): void {
  // 复用既有「新建笔记」对话框并预填建议名（FR-EDITOR-27）：不新增写入路径。
  ops.createFromLink(name);
}

/** 阅读态点标签（FR-TAG-05）：先按单 dock 规则打开标签面板，再选中该标签。 */
function onOpenTag(tag: string): void {
  workspace.openTags();
  tags.selectTag(tag);
  focusTags();
}

/**
 * 阅读态本地附件（E1 / FR-EDITOR-43）：vault root + 当前笔记 relPath → asset:// URL。
 * 解析失败返回 null ⇒ 渲染器保留原 src（远程/绝对路径不生成可请求 URL，SEC-08）。
 */
function resolveAsset(src: string): string | null {
  const root = vaultStore.current?.root;
  const note = editor.activeRelPath;
  return root && note ? assetUrlFor(root, note, src) : null;
}

/** 快速打开只负责「打开文件」，不请求行定位（它不是命中跳转）。 */
function onOpenQuick(relPath: string): void {
  search.closeQuickOpen();
  void editor.openNote(relPath);
}

function parentOf(relPath: string, isDir: boolean): string {
  if (isDir) return relPath;
  const index = relPath.lastIndexOf('/');
  return index < 0 ? '' : relPath.slice(0, index);
}

function onAction(payload: { type: MenuAction; relPath: string; isDir: boolean }): void {
  const parent = parentOf(payload.relPath, payload.isDir);
  if (payload.type === 'newNote') ops.openNewNote(parent);
  else if (payload.type === 'newFolder') ops.openNewFolder(parent);
  else if (payload.type === 'move') ops.openMove(payload.relPath);
  else if (payload.type === 'rename') ops.openRename(payload.relPath);
  else if (payload.type === 'delete') ops.openDelete(payload.relPath);
}
</script>

<template>
  <div class="kp-workspace">
    <p v-if="vaultStore.error" class="kp-workspace__error">{{ vaultStore.error }}</p>
    <header v-if="vaultStore.current" class="kp-workspace__bar">
      <span class="kp-workspace__vault" data-testid="workspace-vault-name">
        {{ vaultStore.current.displayName }}
      </span>
      <button
        type="button"
        class="kp-workspace__switch"
        data-testid="workspace-switch-vault"
        @click="workspace.switchVault()"
      >
        切换知识库
      </button>
    </header>
    <!-- FR-GLOBAL-01 / DEBT-20：全局库重建的一次性告知（正常启动不渲染） -->
    <div v-if="workspace.globalRecovery" class="kp-workspace__recovery" data-testid="global-recovered" role="status">
      <p>
        全局库已重建，请重新选择知识库。笔记数据在 Vault 内，未受影响。
        <span v-if="workspace.globalRecovery.backup">原文件已保留为「{{ workspace.globalRecovery.backup }}」。</span>
      </p>
      <button type="button" data-testid="global-recovered-dismiss" @click="workspace.dismissGlobalRecovery()">
        知道了
      </button>
    </div>
    <p v-if="workspace.switchError" class="kp-workspace__error" data-testid="workspace-switch-error">
      {{ workspace.switchError }}
    </p>
    <div class="kp-workspace__body">
      <Ribbon
        :collapsed="layout.effectiveCollapsed"
        :right-panel-visible="layout.rightPanel === 'links'"
        :search-open="search.panelOpen"
        :tags-open="tags.panelOpen"
        @toggle-sidebar="workspace.toggleSidebar()"
        @search="workspace.toggleSearch()"
        @tags="workspace.toggleTags()"
        @settings="settings.open()"
        @toggle-right="workspace.toggleRightDock()"
      />
      <template v-if="vaultStore.current">
        <aside
          v-show="!layout.effectiveCollapsed"
          class="kp-workspace__side"
          data-testid="workspace-sidebar"
          :style="{ width: ((workspace.draggingWidth ?? layout.sidebarWidth)) + 'px', minWidth: SIDEBAR_MIN_WIDTH + 'px' }"
        >
          <FileTree @open="onOpen" @action="onAction" />
        </aside>
        <div
          v-if="!layout.effectiveCollapsed"
          class="kp-workspace__resizer"
          data-testid="workspace-resizer"
          role="separator"
          aria-orientation="vertical"
          @pointerdown="workspace.startSidebarResize($event)"
        />
        <section class="kp-workspace__main">
          <EditorView
            :open-link="onOpenWikiLink"
            :open-tag="onOpenTag"
            :create-note="onCreateFromLink"
            :resolve-asset="resolveAsset"
          />
        </section>
        <!-- 单一右栏 dock：搜索 / 标签 / 反链同一时刻最多一个（9.6 的并排与重叠问题） -->
        <div v-if="workspace.rightDockPanel !== null" class="kp-workspace__right" data-testid="workspace-right-dock">
          <SearchPanel
            v-if="workspace.rightDockPanel === 'search'"
            ref="searchPanel"
            @open="onOpenHit"
            @close="search.closePanel()"
          />
          <TagPanel
            v-else-if="workspace.rightDockPanel === 'tags'"
            ref="tagPanel"
            data-testid="workspace-tag-panel"
            @open="onOpen"
            @close="tags.closePanel()"
          />
          <LinksPanel v-else :active-rel-path="editor.activeRelPath ?? null" @open="onOpenLink" />
        </div>
      </template>
      <VaultList v-else />
    </div>
    <QuickOpen
      v-if="search.quickOpenVisible"
      ref="quickOpen"
      @open="onOpenQuick"
      @close="search.closeQuickOpen()"
    />
    <FileOpsDialog />
    <CommandPalette />
    <SettingsPanel />
    <p v-if="ops.notice" class="kp-workspace__notice" data-testid="workspace-notice">
      {{ ops.notice }}
    </p>
    <StatusBar :vault="vaultStore.current" />
  </div>
</template>

<style scoped>
.kp-workspace { display: flex; flex-direction: column; height: 100vh; overflow: hidden; }
.kp-workspace__body { flex: 1; display: flex; min-height: 0; min-width: 0; overflow: hidden; }
.kp-workspace__resizer { width: 4px; flex: 0 0 4px; cursor: col-resize; background: var(--kp-border); }
.kp-workspace__resizer:hover { background: var(--kp-accent); }
/* 宽度与最小宽度来自 layout store 常量（内联 style），这里不再写死数值 */
.kp-workspace__side { flex: 0 0 auto; border-right: 1px solid var(--kp-border, #ddd); overflow: hidden; }
/* 编辑区：flex:1 + min-width:0 —— 右栏与侧栏都不得把它挤出视口（9.7） */
.kp-workspace__main { flex: 1; min-width: 0; overflow: hidden; }
.kp-workspace__right { display: flex; flex: 0 0 auto; min-width: 0; overflow: hidden; }
.kp-workspace__right > * { min-width: 0; }
.kp-workspace__recovery { display: flex; align-items: center; gap: 8px; padding: 6px 10px; background: #fff8e1; border-bottom: 1px solid #ffe082; font-size: 13px; }
.kp-workspace__recovery p { margin: 0; flex: 1; }
.kp-workspace__error { margin: 0; padding: 6px 8px; color: #b00020; }
.kp-workspace__notice { position: fixed; bottom: 32px; left: 50%; transform: translateX(-50%); margin: 0; padding: 6px 12px; background: #e8f5e9; border-radius: 4px; font-size: 12px; }
</style>
