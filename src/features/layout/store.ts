/**
 * 布局状态（FR-LAYOUT-01/03/04/05）。
 *
 * **作用域**：PRD FR-LAYOUT-01 原文明确「状态记忆于 `vault_state`」——布局跟着 Vault 走，
 * 不是全局偏好。键沿用 `ui.` 前缀（与批次 3 的 `ui.showHiddenFiles` 同一族）：
 * `ui.sidebarCollapsed` / `ui.sidebarWidth` / `ui.rightPanel`。
 *
 * 读取/写入用注入的 `vaultState` 通路（与 settings 的 AC-SET-02 同法）：store 不认识 IPC，
 * 单测可以给按 Vault 分桶的假实现，直接证明「切 Vault 不串」。
 */
import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

export const SIDEBAR_MIN_WIDTH = 200;
export const SIDEBAR_MAX_WIDTH = 600;
export const SIDEBAR_DEFAULT_WIDTH = 280;
/** FR-LAYOUT-04：窄窗口阈值（< 900px 自动折叠）。 */
export const NARROW_BREAKPOINT = 900;

export const STATE_KEY_SIDEBAR_COLLAPSED = 'ui.sidebarCollapsed';
export const STATE_KEY_SIDEBAR_WIDTH = 'ui.sidebarWidth';
export const STATE_KEY_RIGHT_PANEL = 'ui.rightPanel';

export type RightPanel = 'links' | 'hidden';

export interface VaultStateIO {
  read(key: string): Promise<unknown>;
  write(key: string, value: unknown): Promise<void>;
}

/** 夹取宽度：拖拽与恢复共用同一口径（200–600）。 */
export function clampSidebarWidth(px: number): number {
  if (!Number.isFinite(px)) return SIDEBAR_DEFAULT_WIDTH;
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(px)));
}

export const useLayoutStore = defineStore('layout', () => {
  const sidebarCollapsed = ref(false);
  const sidebarWidth = ref(SIDEBAR_DEFAULT_WIDTH);
  const rightPanel = ref<RightPanel>('links');
  /** FR-LAYOUT-04：视口窄于阈值时自动折叠（用户显式展开仍可用，窄屏优先级更高）。 */
  const narrow = ref(false);
  let io: VaultStateIO | null = null;

  const effectiveCollapsed = computed(() => sidebarCollapsed.value || narrow.value);
  const asideWidth = computed(() => (effectiveCollapsed.value ? 0 : sidebarWidth.value));

  function configureVaultState(next: VaultStateIO | null): void {
    io = next;
  }

  /** 读取当前 Vault 的布局状态（切 Vault 后必须重新调用）。 */
  async function load(): Promise<void> {
    if (io === null) return;
    const collapsed = await io.read(STATE_KEY_SIDEBAR_COLLAPSED);
    if (typeof collapsed === 'boolean') sidebarCollapsed.value = collapsed;
    const width = await io.read(STATE_KEY_SIDEBAR_WIDTH);
    if (typeof width === 'number') sidebarWidth.value = clampSidebarWidth(width);
    const panel = await io.read(STATE_KEY_RIGHT_PANEL);
    if (panel === 'links' || panel === 'hidden') rightPanel.value = panel;
  }

  async function persistCollapsed(value: boolean): Promise<void> {
    if (io !== null) await io.write(STATE_KEY_SIDEBAR_COLLAPSED, value);
  }

  async function toggleSidebar(): Promise<void> {
    sidebarCollapsed.value = !sidebarCollapsed.value;
    await persistCollapsed(sidebarCollapsed.value);
  }

  /** 拖拽结束/恢复时写入；拖拽过程中不落盘（只在 pointerup 调一次）。 */
  async function setSidebarWidth(px: number): Promise<void> {
    sidebarWidth.value = clampSidebarWidth(px);
    if (io !== null) await io.write(STATE_KEY_SIDEBAR_WIDTH, sidebarWidth.value);
  }

  async function toggleRightPanel(): Promise<void> {
    rightPanel.value = rightPanel.value === 'links' ? 'hidden' : 'links';
    if (io !== null) await io.write(STATE_KEY_RIGHT_PANEL, rightPanel.value);
  }

  /** FR-LAYOUT-04：窗口宽度变化时调用（窄屏自动折叠，不写盘 —— 是临时态）。 */
  function applyViewport(width: number): void {
    narrow.value = width < NARROW_BREAKPOINT;
  }

  return {
    sidebarCollapsed, sidebarWidth, rightPanel, narrow,
    effectiveCollapsed, asideWidth,
    configureVaultState, load, toggleSidebar, setSidebarWidth, toggleRightPanel, applyViewport,
  };
});
