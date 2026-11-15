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
import { asKpError } from '@core/ipc/errors';

/** 侧栏宽度范围（真机反馈：200 太窄看不清、600 太宽挤占编辑区）。 */
export const SIDEBAR_MIN_WIDTH = 240;
export const SIDEBAR_MAX_WIDTH = 520;
export const SIDEBAR_DEFAULT_WIDTH = 280;
/** FR-LAYOUT-04：窄窗口阈值（< 900px 自动折叠）。 */
export const NARROW_BREAKPOINT = 900;

export const STATE_KEY_SIDEBAR_COLLAPSED = 'ui.sidebarCollapsed';
export const STATE_KEY_SIDEBAR_WIDTH = 'ui.sidebarWidth';
export const STATE_KEY_RIGHT_PANEL = 'ui.rightPanel';

export type RightPanel = 'links' | 'hidden';

export interface VaultStateIO {
  /**
   * 批量读取：一次 IPC 拿全三个键。
   *
   * 为什么不是「一个键一次读」：切 Vault 是异步的，分开读会在切库瞬间读到「A 的宽度 + B 的面板」
   * —— 这正是真机 9.6「偶尔性的串布局状态」的形态。一次快照保证三个值来自同一个 Vault。
   */
  read(keys: string[]): Promise<Record<string, unknown>>;
  write(key: string, value: unknown): Promise<void>;
}

/** 夹取宽度：拖拽与恢复共用同一口径（240–520）。 */
export function clampSidebarWidth(px: number): number {
  if (!Number.isFinite(px)) return SIDEBAR_DEFAULT_WIDTH;
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(px)));
}

export const useLayoutStore = defineStore('layout', () => {
  const sidebarCollapsed = ref(false);
  const sidebarWidth = ref(SIDEBAR_DEFAULT_WIDTH);
  /**
   * 右侧面板默认**关闭**（真机反馈：反链默认打开会与搜索/标签挤在一起，编辑区被压）。
   * vault_state 里有记录时由 load() 按记录恢复（FR-LAYOUT-05）。
   */
  const rightPanel = ref<RightPanel>('hidden');
  /** FR-LAYOUT-04：视口窄于阈值时自动折叠（用户显式展开仍可用，窄屏优先级更高）。 */
  const narrow = ref(false);
  let io: VaultStateIO | null = null;
  /** 切 Vault 的并发 load 序号：只接受最后一次，慢的旧响应不得覆盖新库（9.6）。 */
  let loadSeq = 0;

  const effectiveCollapsed = computed(() => sidebarCollapsed.value || narrow.value);
  const asideWidth = computed(() => (effectiveCollapsed.value ? 0 : sidebarWidth.value));

  function configureVaultState(next: VaultStateIO | null): void {
    io = next;
  }

  /** 回到默认布局（切 Vault 的瞬间调用；**不写盘**——新库的值由 load() 决定）。 */
  function reset(): void {
    loadSeq += 1; // 作废在途的 load：它的结果属于上一个 Vault
    sidebarCollapsed.value = false;
    sidebarWidth.value = SIDEBAR_DEFAULT_WIDTH;
    rightPanel.value = 'hidden';
  }

  /** 读取当前 Vault 的布局状态（启动与切 Vault 后都必须调用）。 */
  async function load(): Promise<void> {
    if (io === null) return;
    const seq = ++loadSeq;
    try {
      const values = await io.read([STATE_KEY_SIDEBAR_COLLAPSED, STATE_KEY_SIDEBAR_WIDTH, STATE_KEY_RIGHT_PANEL]);
      if (seq !== loadSeq) return; // 过期响应：整包丢弃
      const collapsed = values[STATE_KEY_SIDEBAR_COLLAPSED];
      sidebarCollapsed.value = typeof collapsed === 'boolean' ? collapsed : false;
      const width = values[STATE_KEY_SIDEBAR_WIDTH];
      sidebarWidth.value = typeof width === 'number' ? clampSidebarWidth(width) : SIDEBAR_DEFAULT_WIDTH;
      const panel = values[STATE_KEY_RIGHT_PANEL];
      rightPanel.value = panel === 'links' ? 'links' : 'hidden';
    } catch (err) {
      // 布局是非关键状态：读不到就用默认值，但必须留痕（R-15：不空吞）
      console.warn('布局状态读取失败，使用默认布局', asKpError(err).message);
    }
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

  /** 右侧面板的显式设置（dock 编排在 layout/store 之外的 workspace store 里）。 */
  async function setRightPanel(panel: RightPanel): Promise<void> {
    if (rightPanel.value === panel) return;
    rightPanel.value = panel;
    if (io !== null) await io.write(STATE_KEY_RIGHT_PANEL, panel);
  }

  async function toggleRightPanel(): Promise<void> {
    await setRightPanel(rightPanel.value === 'links' ? 'hidden' : 'links');
  }

  /** FR-LAYOUT-04：窗口宽度变化时调用（窄屏自动折叠，不写盘 —— 是临时态）。 */
  function applyViewport(width: number): void {
    narrow.value = width < NARROW_BREAKPOINT;
  }

  return {
    sidebarCollapsed, sidebarWidth, rightPanel, narrow,
    effectiveCollapsed, asideWidth,
    configureVaultState, reset, load, toggleSidebar, setSidebarWidth, setRightPanel, toggleRightPanel,
    applyViewport,
  };
});
