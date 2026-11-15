/**
 * 工作区编排 store（布局域的应用层编排点；FE-02：跨 feature 的编排集中在布局层）。
 *
 * 为什么集中在这里而不是散在 WorkspaceLayout.vue：真机缺陷 9.5/9.6/7.5 都出在
 * 「多个 feature 的时序与重置」上，必须有一处可单测的唯一编排点；组件只做渲染与组件 ref。
 *
 * 三件事：
 * ① 右侧单一 dock —— 搜索 / 标签 / 反链同一时刻最多一个（并排会与编辑区重叠）；
 * ② 切 Vault 的作用域切换 —— 清空所有跨库状态后再恢复新库的持久化布局；
 * ③ 命令面板执行体 —— 失败必须抛（7.5：面板保持打开并显示红字）。
 */
import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import { systemInfo } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import { clearRenderCache } from '@core/markdown/renderer';
import { useEditorStore } from '@features/editor';
import { useFileOpsStore, useFileTreeStore } from '@features/file-tree';
import { useLinksStore } from '@features/links';
import { useSearchStore } from '@features/search';
import { useSettingsStore } from '@features/settings';
import { useTagsStore } from '@features/tags';
import { useVaultStore } from '@features/vault';
import { useLayoutStore } from '../store';
import { beginSidebarResize, watchViewport } from '../viewport';

/** 右栏当前可见的面板（单 dock：至多一个非 null）。 */
export type RightDockPanel = 'search' | 'tags' | 'links' | null;

export const useWorkspaceStore = defineStore('workspace', () => {
  const layout = useLayoutStore();
  const editor = useEditorStore();
  const search = useSearchStore();
  const tags = useTagsStore();
  const links = useLinksStore();
  const fileTree = useFileTreeStore();
  const ops = useFileOpsStore();
  const settings = useSettingsStore();
  const vault = useVaultStore();

  /** 切换失败时的可见提示（R-07：宁可不切，也不静默丢内容）。 */
  const switchError = ref<string | null>(null);
  /** FR-GLOBAL-01 / DEBT-20：全局库本次启动被重建过时的一次性告知。 */
  const globalRecovery = ref<{ backup: string | null; reason: string } | null>(null);
  /** 拖拽中的临时宽度（不落盘；pointerup 才写 vault_state）。 */
  const draggingWidth = ref<number | null>(null);
  /** 切 Vault 请求序号：只接受最后一次，快速来回切换时旧响应不得覆盖新库（9.6）。 */
  let scopeSeq = 0;

  /** 当前可见的右栏面板（互斥由下面的 open* 函数保证，这里是唯一的渲染依据）。 */
  const rightDockPanel = computed<RightDockPanel>(() => {
    if (search.panelOpen) return 'search';
    if (tags.panelOpen) return 'tags';
    return layout.rightPanel === 'links' ? 'links' : null;
  });

  function closePanels(): void {
    search.closePanel();
    tags.closePanel();
  }

  /** 打开搜索：关闭标签面板 + 隐藏反链并**持久化**（否则重启后三栏重叠，9.6）。 */
  function openSearch(): void {
    tags.closePanel();
    void layout.setRightPanel('hidden');
    search.openPanel();
  }

  function toggleSearch(): void {
    if (search.panelOpen) search.closePanel();
    else openSearch();
  }

  function openTags(): void {
    search.closePanel();
    void layout.setRightPanel('hidden');
    tags.openPanel();
  }

  function toggleTags(): void {
    if (tags.panelOpen) tags.closePanel();
    else openTags();
  }

  /** Ribbon ▥ / 命令「切换右侧面板」：反链可见 → 全部隐藏；否则关搜索/标签并显示反链。 */
  function toggleRightDock(): void {
    if (layout.rightPanel === 'links') {
      void layout.setRightPanel('hidden');
      return;
    }
    closePanels();
    void layout.setRightPanel('links');
  }

  /** 侧栏拖拽：预览在组件层，松手落盘一次（夹取与监听注销在 viewport 模块）。 */
  function startSidebarResize(event: PointerEvent): void {
    if (layout.effectiveCollapsed) return;
    beginSidebarResize(
      event,
      layout.sidebarWidth,
      (width) => { draggingWidth.value = width; },
      (width) => { draggingWidth.value = null; void layout.setSidebarWidth(width); },
    );
  }

  /** FR-LAYOUT-04：resize 监听只在挂载时注册一次、卸载时注销（此前每次切库都重复注册且从不注销）。 */
  function trackViewport(): () => void {
    return watchViewport((width) => layout.applyViewport(width));
  }

  /** 用户关掉「全局库已重建」横幅（只提示一次）。 */
  function dismissGlobalRecovery(): void {
    globalRecovery.value = null;
  }

  /** DEBT-20：全局库重建的一次性告知（启动早期发生，webview 就绪后主动取一次）。 */
  async function checkGlobalRecovery(): Promise<void> {
    try {
      const info = await systemInfo();
      if (info.globalDbRebuilt === true) {
        globalRecovery.value = { backup: info.globalDbBackup ?? null, reason: info.globalDbReason ?? '' };
      }
    } catch (err) {
      // 诊断信息取不到不影响主流程；但也不静默丢错误（R-15）
      console.warn('全局库状态读取失败（不影响主流程）', asKpError(err).message);
    }
  }

  /**
   * 切 Vault 的作用域切换（9.5/9.6 的核心）。
   *
   * 顺序刻意如此：
   * ① 先回默认布局并清空**所有**跨 Vault 状态——编辑器标签（串文件）、文件树展开/缓存
   *    （串目录，即「文件不存在：归档 不是目录」的根因）、操作提示（串提示）、面板与渲染缓存；
   * ② 新 Vault 就绪后再恢复它自己的持久化布局/设置；
   * ③ 序号保证快速切换时只有最后一次生效（旧响应不得把新库状态改回去）。
   */
  async function applyVaultScope(vaultId: number | null): Promise<void> {
    const seq = ++scopeSeq;
    layout.reset();
    switchError.value = null;
    globalRecovery.value = null;
    closePanels();
    editor.closeAll();
    links.reset();
    // 串目录（「文件不存在：归档 不是目录」）与串提示的根因：A 库的展开路径/隐藏开关/操作提示会留在
    // store 里，而 restoreState 是并集语义。两个 reset 由 file-tree 域提供（不写 vault_state）。
    fileTree.reset();
    ops.clearNotice();
    clearRenderCache();
    if (vaultId === null) return; // 回到欢迎页：没有「新库」可恢复
    await layout.load();
    if (seq !== scopeSeq) return;
    await settings.loadVaultScope(); // 同一次调用里按新库 restoreState 文件树（见布局组件的注入）
    if (seq !== scopeSeq) return;
    layout.applyViewport(window.innerWidth);
    // 反链面板若本来就开着（A→B 直接切换时它不会重新挂载），悬空/孤立清单必须按新库重查。
    if (layout.rightPanel === 'links') void links.refresh();
  }

  /**
   * FR-VAULT-04/05：回到欢迎页以切换知识库。
   * **先刷盘再关闭**：仍有标签保存失败则取消切换并给出可见提示（宁可不切，也不丢内容）。
   */
  async function switchVault(): Promise<void> {
    switchError.value = null;
    await editor.saveAll();
    if (editor.hasUnsaved()) {
      const dirty = editor.buffers.filter((b) => editor.isDirty(b.relPath)).length;
      switchError.value = `仍有 ${dirty} 个标签保存失败，已取消切换；请处理后重试。`;
      return;
    }
    await vault.closeCurrent();
  }

  // ── 命令面板执行体（7.5：保存失败必须 throw —— palette.store 据此保持打开并显示红字）──
  async function saveNote(): Promise<void> {
    const relPath = editor.activeRelPath;
    if (relPath === null) return;
    await editor.save(relPath);
    // editor.save 把错误收进 buffer.error 后**正常 resolve**：不检查就会让命令面板误以为成功并关闭。
    const buffer = editor.buffers.find((b) => b.relPath === relPath);
    if (buffer?.error) throw new Error(buffer.error);
  }

  async function saveAll(): Promise<void> {
    await editor.saveAll();
    const failed = editor.buffers.filter((b) => b.error !== null);
    if (failed.length > 0) throw new Error(`有 ${failed.length} 个标签保存失败：${failed[0].error}`);
  }

  function closeNote(): void {
    const relPath = editor.activeRelPath;
    if (relPath !== null) editor.closeNote(relPath);
  }

  function toggleMode(): void {
    editor.setMode(editor.mode === 'edit' ? 'read' : 'edit');
  }

  function newNote(): void {
    ops.openNewNote('');
  }

  function quickOpen(): void {
    search.openQuickOpen();
  }

  async function refreshTree(): Promise<void> {
    await fileTree.refresh();
  }

  async function toggleHidden(): Promise<void> {
    await fileTree.setIncludeHidden(!fileTree.includeHidden);
  }

  function openSettings(): void {
    settings.open();
  }

  function toggleTheme(): void {
    settings.toggleTheme();
  }

  async function toggleSidebar(): Promise<void> {
    await layout.toggleSidebar();
  }

  return {
    switchError, globalRecovery, draggingWidth, rightDockPanel,
    closePanels, openSearch, toggleSearch, openTags, toggleTags, toggleRightDock,
    startSidebarResize, trackViewport, checkGlobalRecovery, dismissGlobalRecovery, applyVaultScope, switchVault,
    saveNote, saveAll, closeNote, toggleMode, newNote, quickOpen, refreshTree, toggleHidden,
    openSettings, toggleTheme, toggleSidebar,
  };
});
