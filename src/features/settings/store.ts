/**
 * 设置页状态（FR-SET-01/03/07/20/21）。
 *
 * 作用域（FR-SET-04）：本轮只做「全局」（preference 表）；Vault 级设置只展示默认值与作用域，
 * 不假装能改（AC-SET-02 的隔离属 P1，本轮未做）。
 *
 * 启动时序：外观必须在**首次渲染前**应用到 document —— 由 `app/bootstrap.ts` 在 mount 前 await
 * `init()`（真机 8.6 的根因正是 init 从没被调用，重启后主题/字号/缩放全回默认）。
 */
import { computed, ref, shallowRef } from 'vue';
import { defineStore } from 'pinia';
import { asKpError } from '@core/ipc/errors';
import { preferenceGet } from '@core/ipc/commands';
import { schedulePreferenceSet } from '@core/ipc/preferences';
import {
  applyEditorAppearance, applyTheme, DEFAULT_THEME, FONT_SIZE_MAX, FONT_SIZE_MIN, loadTheme,
  PREF_FONT_SIZE, PREF_ZOOM, persistTheme, watchSystemTheme, type ThemeMode, ZOOM_MAX, ZOOM_MIN,
} from '@core/theme';
import { AUTOSAVE_DEFAULT_MS, clampAutosaveDelay, persistAutosaveDelay, readAutosaveDelay } from '@features/editor/stores/editor_settings';

/**
 * 自动保存防抖的读写注入。
 *
 * 为什么需要注入而不是直接 import 编辑器 store：FR-SET 域与编辑器域是平级 feature（FE-02 禁止横向依赖），
 * 且「自动保存定时器真正读哪个值」只有编辑器知道 —— 由布局层把两侧接成**单一来源**，
 * 否则设置面板改了值、编辑器定时器还读旧值（真机 S2 的"改了 2000 重启又回 1000"同源）。
 */
interface AutosaveScope {
  get(): number;
  set(ms: number): void;
}

/** 系统主题监听的注册/注销编排：只有 system 模式挂监听，其余模式必须注销（系统变化不得覆盖手选）。 */
function createSystemThemeSync(isSystemMode: () => boolean): { sync(): void } {
  let unwatch: (() => void) | null = null;
  return {
    sync(): void {
      unwatch?.();
      unwatch = null;
      if (!isSystemMode()) return;
      unwatch = watchSystemTheme(() => {
        // 监听已随模式注销；这里再判一次，防「注销与事件派发同帧」的竞态覆盖用户手选。
        if (isSystemMode()) applyTheme('system');
      });
    },
  };
}

/** 把库里读到的数夹取到允许区间；非数字/缺失保持当前值（坏值不能撑坏界面）。 */
function clampStored(current: number, raw: unknown, min: number, max: number): number {
  return typeof raw === 'number' ? Math.min(max, Math.max(min, Math.round(raw))) : current;
}

export const useSettingsStore = defineStore('settings', () => {
  const isOpen = ref(false);
  const theme = ref<ThemeMode>(DEFAULT_THEME);
  const fontSize = ref(16);
  const zoom = ref(100);
  /** 未接线时的回退值；接线后对外展示值以编辑器 store 为准（见 autosaveDelayMs）。 */
  const autosaveFallbackMs = ref(AUTOSAVE_DEFAULT_MS);
  const autosaveScope = shallowRef<AutosaveScope | null>(null);
  /** system 模式系统配色监听的注册/注销入口（init 与 setTheme 共用；幂等）。 */
  const systemThemeSync = createSystemThemeSync(() => theme.value === 'system');
  /** Vault 级设置（AC-SET-02）：由布局层注入「读/写当前 Vault 的 ui.showHiddenFiles」，store 不认识 vault_state。 */
  const showHidden = ref(false);
  let vaultScope: { read(): Promise<boolean>; write(value: boolean): Promise<void> } | null = null;

  function configureVaultScope(deps: { read(): Promise<boolean>; write(value: boolean): Promise<void> }): void {
    vaultScope = deps;
  }

  /** 读取当前 Vault 的作用域设置（切 Vault 后必须重新调用；不缓存跨 Vault 的值）。 */
  async function loadVaultScope(): Promise<void> {
    showHidden.value = vaultScope === null ? false : await vaultScope.read();
  }

  /** 写 Vault 级设置：走注入的 vault_state 通路（**不能**写 preference，否则会串 Vault）。 */
  async function setShowHidden(next: boolean): Promise<void> {
    showHidden.value = next;
    if (vaultScope !== null) await vaultScope.write(next);
  }

  function open(): void { isOpen.value = true; }
  function close(): void { isOpen.value = false; }

  async function init(): Promise<void> {
    theme.value = await loadTheme();
    applyTheme(theme.value);
    systemThemeSync.sync();
    try {
      const stored = await preferenceGet([PREF_FONT_SIZE, PREF_ZOOM]);
      fontSize.value = clampStored(fontSize.value, stored.values[PREF_FONT_SIZE], FONT_SIZE_MIN, FONT_SIZE_MAX);
      zoom.value = clampStored(zoom.value, stored.values[PREF_ZOOM], ZOOM_MIN, ZOOM_MAX);
      applyEditorAppearance({ fontSize: fontSize.value, zoom: zoom.value });
    } catch (err) {
      console.warn('外观设置读取失败，使用默认值', asKpError(err).message);
    }
    const autosave = await readAutosaveDelay();
    if (autosave !== null) autosaveFallbackMs.value = autosave;
  }

  /** 把设置面板的自动保存控件接到编辑器的真实定时器来源（由布局层调用一次）。 */
  function configureAutosave(deps: AutosaveScope): void {
    autosaveScope.value = deps;
  }

  /** 对外展示值：接线后读编辑器当前值（工具栏与设置面板同源）；未接线时用本地回退值。 */
  const autosaveDelayMs = computed<number>(() => {
    const scope = autosaveScope.value;
    return scope === null ? autosaveFallbackMs.value : clampAutosaveDelay(scope.get());
  });

  function setTheme(next: ThemeMode): void {
    theme.value = next;
    applyTheme(next);
    persistTheme(next);
    systemThemeSync.sync();
  }

  /** 「切换主题」命令：在 light/dark 间切换（system 按其解析结果取反）。 */
  function toggleTheme(): void {
    const current = document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
    setTheme(current === 'dark' ? 'light' : 'dark');
  }

  function setFontSize(px: number): void {
    const next = Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, Math.round(px)));
    fontSize.value = next;
    applyEditorAppearance({ fontSize: next });
    schedulePreferenceSet({ [PREF_FONT_SIZE]: next });
  }

  function setZoom(percent: number): void {
    const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(percent)));
    zoom.value = next;
    applyEditorAppearance({ zoom: next });
    schedulePreferenceSet({ [PREF_ZOOM]: next });
  }

  function setAutosaveDelay(ms: number): void {
    const next = clampAutosaveDelay(ms);
    autosaveFallbackMs.value = next;
    const scope = autosaveScope.value;
    // 接线后由编辑器 store 唯一落盘（它自己的 setter 会持久化），避免两处各写一份偏好。
    if (scope !== null) scope.set(next);
    else persistAutosaveDelay(next);
  }

  return {
    isOpen, theme, fontSize, zoom, autosaveDelayMs, showHidden,
    configureVaultScope, configureAutosave, loadVaultScope, setShowHidden,
    open, close, init, setTheme, toggleTheme, setFontSize, setZoom, setAutosaveDelay,
  };
});
