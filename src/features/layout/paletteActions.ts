import type { PaletteActions } from '@features/palette';
import type { useWorkspaceStore } from './stores/workspace';

type WorkspaceStore = ReturnType<typeof useWorkspaceStore>;

/** 面板聚焦需要组件 ref（store 不持有 DOM），由布局组件注入。 */
export interface PaletteFocusPorts {
  quickOpen(): void;
  search(): void;
  tags(): void;
}

/**
 * 命令面板执行体（FR-PALETTE-01/03/07 与新增的布局命令）。
 *
 * 只做「命令 → 既有通路」的映射：不新增写路径（R-01 的同类约束），也不在这里做业务判断。
 * 保存类动作的失败语义由 workspace store 保证 7.5：`save` 失败会 throw，palette.execute
 * 捕获后**保持面板打开**并显示红字（此前 save 内部吞错 ⇒ 面板误以为成功而关闭）。
 */
export function createPaletteActions(workspace: WorkspaceStore, focus: PaletteFocusPorts): PaletteActions {
  return {
    newNote: () => workspace.newNote(),
    quickOpen: () => { workspace.quickOpen(); focus.quickOpen(); },
    search: () => { workspace.openSearch(); focus.search(); },
    tags: () => { workspace.openTags(); focus.tags(); },
    saveNote: () => workspace.saveNote(),
    saveAll: () => workspace.saveAll(),
    closeNote: () => workspace.closeNote(),
    toggleMode: () => workspace.toggleMode(),
    switchVault: () => workspace.switchVault(),
    refreshTree: () => workspace.refreshTree(),
    toggleHidden: () => workspace.toggleHidden(),
    openSettings: () => workspace.openSettings(),
    toggleTheme: () => workspace.toggleTheme(),
    toggleSidebar: () => workspace.toggleSidebar(),
    toggleRightPanel: () => workspace.toggleRightDock(),
  };
}
