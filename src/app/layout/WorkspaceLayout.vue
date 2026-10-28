<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { systemInfo, vaultStateGet, vaultStateSet } from '@core/ipc/commands';
import { bindShortcuts } from '@core/shortcut';
import { useVaultStore } from '@features/vault';
import { EditorView, useEditorStore } from '@features/editor';
import { FileOpsDialog, FileTree, useFileOpsStore, type MenuAction } from '@features/file-tree';
import { LinksPanel } from '@features/links';
import { QuickOpen, SearchPanel, useSearchStore } from '@features/search';
import { TagPanel, useTagsStore } from '@features/tags';
import { CommandPalette, usePaletteStore } from '@features/palette';
import { SettingsPanel, useSettingsStore } from '@features/settings';
import { Ribbon, clampSidebarWidth, useLayoutStore } from '@features/layout';
import { useFileTreeStore } from '@features/file-tree/stores/fileTree';
import VaultList from '@features/vault/components/VaultList.vue';
import StatusBar from './StatusBar.vue';

/**
 * 工作区布局（技术方案 §2.2 的 `app/layout`）：左文件树 + 右编辑器 + 文件操作对话框 + 状态栏。
 *
 * 接线职责（M2 的最后一公里）：
 * - 文件树点开笔记 → 编辑器打开（FR-FILE-10 的进入编辑态）；
 * - 文件树右键动作 → 文件操作对话框（新建/重命名/删除，AC-FILE-03/04）；
 * - 重命名成功后由 fileOps store 调 `editor.followRename`（FR-FILE-28 / AC-FILE-05）；
 * - 状态栏显示 Vault 名称与索引状态（FR-VAULT-11）与 Git 忽略提示（FR-VAULT-12/FR-STORAGE-03）。
 * 未打开 Vault 时展示 Vault 列表（避免空白工作区）。
 */
const vaultStore = useVaultStore();
const ops = useFileOpsStore();
const editor = useEditorStore();
const search = useSearchStore();
const tags = useTagsStore();
const palette = usePaletteStore();
const fileTree = useFileTreeStore();
const settings = useSettingsStore();
const layout = useLayoutStore();
/** 拖拽中的临时宽度（不每次落盘；pointerup 才写 vault_state）。 */
const draggingWidth = ref<number | null>(null);
const searchPanel = ref<InstanceType<typeof SearchPanel> | null>(null);
const quickOpen = ref<InstanceType<typeof QuickOpen> | null>(null);
const tagPanel = ref<InstanceType<typeof TagPanel> | null>(null);
let unbind: (() => void) | null = null;

/**
 * **FR-GLOBAL-01 / DEBT-20**：全局库在本次启动被重建过时，提示一次（可关闭）。
 *
 * 为什么用 `system_info` 而不是事件：重建发生在**启动早期**（webview 还没就绪），事件发出去也收不到；
 * 恢复信息是"每次启动一个值"，挂载时取一次正好，且正常启动恒为 false（不产生噪声）。
 */
const globalRecovery = ref<{ backup: string | null; reason: string } | null>(null);

async function checkGlobalRecovery(): Promise<void> {
  try {
    const info = await systemInfo();
    if (info.globalDbRebuilt === true) {
      globalRecovery.value = { backup: info.globalDbBackup ?? null, reason: info.globalDbReason ?? '' };
    }
  } catch {
    // 诊断信息取不到不影响主流程（也不静默改写别的状态）
  }
}

// Vault 状态来自 store（唯一来源）：切换/打开/移除都会响应式传播，
// 状态栏的 watch 因此真正生效（M1/M2 复核：此前只在挂载时取一次）。
// 顺带绑定搜索入口（FR-SEARCH-01 的 Mod+Shift+F 与 FR-SEARCH-40 的 Mod+O）——
// 快捷键统一走 core/shortcut（ED-04：组件不自行绑 keydown），卸载时必须注销（ED-01）。
// 命令面板（FR-PALETTE-01/03/07）：执行体在这一层注入（跨 feature 的编排只允许在 layout 层，FE-02）。
// 命令只调用**既有**通路，不新增写路径；不可用命令由 registry 的 needs/reason 判定（FR-PALETTE-08 的机制面）。
// FR-LAYOUT-01：布局状态按 PRD 记在 vault_state（键 ui.sidebarCollapsed / ui.sidebarWidth / ui.rightPanel）。
layout.configureVaultState({
  read: async (key) => (await vaultStateGet([key])).values[key],
  write: async (key, value) => {
    await vaultStateSet({ [key]: value });
  },
});
watch(
  () => vaultStore.current?.vaultId ?? null,
  () => {
    void layout.load();
  },
);
function onViewportResize(): void {
  layout.applyViewport(window.innerWidth);
}
function startResize(event: PointerEvent): void {
  if (layout.effectiveCollapsed) return;
  event.preventDefault();
  const startX = event.clientX;
  const startWidth = layout.sidebarWidth;
  const onMove = (ev: PointerEvent): void => {
    draggingWidth.value = clampSidebarWidth(startWidth + (ev.clientX - startX));
  };
  const onUp = (): void => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    if (draggingWidth.value !== null) {
      const width = draggingWidth.value;
      draggingWidth.value = null;
      void layout.setSidebarWidth(width);
    }
  };
  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
}

// AC-SET-02：Vault 级设置只认「当前 Vault」——读写都走 file-tree 的 vault_state 通路
// （ui.showHiddenFiles 已是 vault_state 作用域，见 fileTree.ts 的 restoreState/setIncludeHidden）。
settings.configureVaultScope({
  read: async () => {
    await fileTree.restoreState();
    return fileTree.includeHidden;
  },
  write: async (value) => {
    await fileTree.setIncludeHidden(value);
  },
});
watch(
  () => vaultStore.current?.vaultId ?? null,
  () => {
    void settings.loadVaultScope();
    void layout.load();
    layout.applyViewport(window.innerWidth);
    window.addEventListener('resize', onViewportResize);
  },
);

palette.configure({
  newNote: () => ops.openNewNote(''),
  quickOpen: () => {
    search.openQuickOpen();
    void nextTick(() => {
      quickOpen.value?.focusInput();
    });
  },
  search: () => {
    search.openPanel();
    void nextTick(() => {
      searchPanel.value?.focusInput();
    });
  },
  tags: () => {
    tags.openPanel();
    void nextTick(() => {
      tagPanel.value?.focusFilter();
    });
  },
  saveNote: () => {
    const relPath = editor.activeRelPath;
    if (relPath !== null) return editor.save(relPath);
  },
  saveAll: () => { void editor.saveAll(); },
  closeNote: () => {
    const relPath = editor.activeRelPath;
    if (relPath !== null) editor.closeNote(relPath);
  },
  toggleMode: () => {
    editor.mode = editor.mode === 'edit' ? 'read' : 'edit';
  },
  switchVault: () => { void onSwitchVault(); },
  refreshTree: () => { void fileTree.refresh(); },
  toggleHidden: () => { void fileTree.setIncludeHidden(!fileTree.includeHidden); },
  openSettings: () => settings.open(),
  toggleSidebar: () => layout.toggleSidebar(),
  toggleRightPanel: () => layout.toggleRightPanel(),
  toggleTheme: () => settings.toggleTheme(),
});

onMounted(() => {
  void vaultStore.init();
  void checkGlobalRecovery();
  unbind = bindShortcuts(window, [
    {
      spec: 'Mod+Shift+F',
      handler: () => {
        search.openPanel();
        void nextTick(() => {
          searchPanel.value?.focusInput();
        });
      },
    },
    {
      spec: 'Mod+O',
      handler: () => {
        search.openQuickOpen();
        void nextTick(() => {
          quickOpen.value?.focusInput();
        });
      },
    },
      {
        // 命令面板（FR-PALETTE-01）：打开时刷新上下文（编辑器/焦点/隐藏文件），置灰判定按当前状态算。
        spec: 'Mod+P',
        handler: () => {
          palette.open({
            hasActiveNote: editor.activeRelPath !== null,
            hasVault: vaultStore.current !== null,
            mode: editor.mode === 'read' ? 'read' : 'edit',
            includeHidden: fileTree.includeHidden,
          });
        },
      },
    {
      // 标签面板（FR-TAG-01 的入口）：与搜索同一形态 —— 先开面板，再聚焦过滤框。
      // 键位选 Mod+Shift+T：Mod+Shift+F 已给搜索、Mod+O 给快速打开，避开二者。
      spec: 'Mod+Shift+T',
      handler: () => {
        tags.openPanel();
        void nextTick(() => {
          tagPanel.value?.focusFilter();
        });
      },
    },
  ]);
});

onUnmounted(() => {
  unbind?.();
  unbind = null;
});

/** 切换失败时的可见提示（R-07：不静默丢内容，也不假装成功）。 */
const switchError = ref<string | null>(null);

/**
 * FR-VAULT-04/05：回到欢迎页以切换知识库。
 *
 * 顺序很关键：**先刷盘，再关闭** —— 否则未保存内容会随 Vault 关闭一起消失
 * （AC-VAULT-03 的"编辑内容被写入磁盘"也正是这条路径）。
 * 若仍有标签保存失败，则**取消切换**并给出可见提示：宁可不切，也不丢内容。
 */
async function onSwitchVault(): Promise<void> {
  switchError.value = null;
  await editor.saveAll();
  if (editor.hasUnsaved()) {
    // 脏判定用 store 的派生函数（buffer 本身没有 isDirty 字段）。
    const dirty = editor.buffers.filter((b) => editor.isDirty(b.relPath)).length;
    switchError.value = `仍有 ${dirty} 个标签保存失败，已取消切换；请处理后重试。`;
    return;
  }
  await vaultStore.closeCurrent();
}

function onOpen(relPath: string): void {
  void editor.openNote(relPath);
}

/** 点击反链/孤立项：打开来源笔记（FR-LINK-13 的第一半；滚动定位与高亮见遗留项）。 */
function onOpenLink(payload: { relPath: string; line: number }): void {
  void editor.reveal(payload.relPath, payload.line);
}

/**
 * 点击搜索结果：同一条既有通路（FR-SEARCH-07 的"跳转并定位到命中位置"）。
 * 带上命中列区间 ⇒ 精确选中命中处；没有区间时编辑器回退成整行选中。
 */
function onOpenHit(relPath: string, line: number, range?: { start: number; end: number }): void {
  void editor.reveal(relPath, line, range);
}

/**
 * 阅读态 wikilink 的跳转出口（FR-EDITOR-25）—— **跨 feature 的编排只在这一层**（FE-02）。
 *
 * 已裁决的链接：与反链/搜索**同一条既有通路** `editor.reveal`；
 * **悬空链接的创建**：交给 file-tree 域的既有"新建笔记"流程（带建议名，新增能力见 `fileOps.createFromLink`）；
 * **标签**：打开标签面板并选中它（对接 PR-C 的 tags store）。
 */
function onOpenWikiLink(target: string, event?: MouseEvent): void {
  // FR-EDITOR-25：Ctrl/Cmd+Alt+点击 ⇒ 新标签页打开（不切走当前标签）
  if (event?.altKey === true) void editor.openNoteInNewTab(target);
  else void editor.reveal(target, 1);
}

function onCreateFromLink(name: string): void {
  // 复用既有的"新建笔记"对话框并**预填**建议名（FR-EDITOR-27 的创建流程）：
  // 不新增写入路径 —— 名称校验、冲突三选项、创建后进入编辑态全部照旧（AC-FILE-03/04）。
  ops.createFromLink(name);
}

function onOpenTag(tag: string): void {
  tags.selectTag(tag);
  void nextTick(() => {
    tagPanel.value?.focusFilter();
  });
}

/** 快速打开只负责"打开文件"，不请求行定位（它不是命中跳转）。 */
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
        @click="onSwitchVault"
      >
        切换知识库
      </button>
    </header>
    <!-- FR-GLOBAL-01 / DEBT-20：全局库重建的一次性告知（正常启动不渲染） -->
    <div v-if="globalRecovery" class="kp-workspace__recovery" data-testid="global-recovered" role="status">
      <p>
        全局库已重建，请重新选择知识库。笔记数据在 Vault 内，未受影响。
        <span v-if="globalRecovery.backup">原文件已保留为「{{ globalRecovery.backup }}」。</span>
      </p>
      <button
        type="button"
        data-testid="global-recovered-dismiss"
        @click="globalRecovery = null"
      >
        知道了
      </button>
    </div>
    <p v-if="switchError" class="kp-workspace__error" data-testid="workspace-switch-error">
      {{ switchError }}
    </p>
    <div class="kp-workspace__body">
      <Ribbon
        :collapsed="layout.effectiveCollapsed"
        :right-panel-visible="layout.rightPanel === 'links'"
        @toggle-sidebar="layout.toggleSidebar()"
        @search="search.openPanel()"
        @tags="tags.openPanel()"
        @settings="settings.open()"
        @toggle-right="layout.toggleRightPanel()"
      />
      <template v-if="vaultStore.current">
        <aside v-show="!layout.effectiveCollapsed" class="kp-workspace__side" data-testid="workspace-sidebar" :style="{ width: ((draggingWidth ?? layout.sidebarWidth)) + 'px' }">
          <FileTree @open="onOpen" @action="onAction" />
        </aside>
        <div
          v-if="!layout.effectiveCollapsed"
          class="kp-workspace__resizer"
          data-testid="workspace-resizer"
          role="separator"
          aria-orientation="vertical"
          @pointerdown="startResize"
        />
        <section class="kp-workspace__main">
          <EditorView
            :open-link="onOpenWikiLink"
            :open-tag="onOpenTag"
            :create-note="onCreateFromLink"
          />
        </section>
        <LinksPanel v-if="layout.rightPanel === 'links'" :active-rel-path="editor.activeRelPath ?? null" @open="onOpenLink" />
        <SearchPanel
          v-if="search.panelOpen"
          ref="searchPanel"
          @open="onOpenHit"
          @close="search.closePanel()"
        />
        <TagPanel
          v-if="tags.panelOpen"
          ref="tagPanel"
          data-testid="workspace-tag-panel"
          @open="onOpen"
          @close="tags.closePanel()"
        />
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
.kp-workspace { display: flex; flex-direction: column; height: 100vh; }
.kp-workspace__body { flex: 1; display: flex; min-height: 0; }
.kp-workspace__resizer { width: 4px; flex: 0 0 4px; cursor: col-resize; background: var(--kp-border); }
.kp-workspace__resizer:hover { background: var(--kp-accent); }
.kp-workspace__side { width: 280px; min-width: 200px; border-right: 1px solid var(--kp-border, #ddd); overflow: hidden; }
.kp-workspace__main { flex: 1; min-width: 0; }
.kp-workspace__recovery { display: flex; align-items: center; gap: 8px; padding: 6px 10px; background: #fff8e1; border-bottom: 1px solid #ffe082; font-size: 13px; }
.kp-workspace__recovery p { margin: 0; flex: 1; }
.kp-workspace__error { margin: 0; padding: 6px 8px; color: #b00020; }
.kp-workspace__notice { position: fixed; bottom: 32px; left: 50%; transform: translateX(-50%); margin: 0; padding: 6px 12px; background: #e8f5e9; border-radius: 4px; font-size: 12px; }
</style>
