/**
 * 设置页状态（FR-SET-01/03/07/20/21）。
 *
 * 作用域（FR-SET-04）：本轮只做「全局」（preference 表）；Vault 级设置只展示默认值与作用域，
 * 不假装能改（AC-SET-02 的隔离属 P1，本轮未做）。
 */
import { ref } from 'vue';
import { defineStore } from 'pinia';
import { asKpError } from '@core/ipc/errors';
import { preferenceGet } from '@core/ipc/commands';
import { schedulePreferenceSet } from '@core/ipc/preferences';
import {
  applyEditorAppearance, applyTheme, DEFAULT_THEME, FONT_SIZE_MAX, FONT_SIZE_MIN, loadTheme,
  PREF_FONT_SIZE, PREF_ZOOM, persistTheme, type ThemeMode, ZOOM_MAX, ZOOM_MIN,
} from '@core/theme';
import { AUTOSAVE_DEFAULT_MS, clampAutosaveDelay, persistAutosaveDelay, readAutosaveDelay } from '@features/editor/stores/editor_settings';

export const useSettingsStore = defineStore('settings', () => {
  const isOpen = ref(false);
  const theme = ref<ThemeMode>(DEFAULT_THEME);
  const fontSize = ref(16);
  const zoom = ref(100);
  const autosaveDelayMs = ref(AUTOSAVE_DEFAULT_MS);
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
    try {
      const stored = await preferenceGet([PREF_FONT_SIZE, PREF_ZOOM]);
      const fs = stored.values[PREF_FONT_SIZE];
      if (typeof fs === 'number') fontSize.value = Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, Math.round(fs)));
      const z = stored.values[PREF_ZOOM];
      if (typeof z === 'number') zoom.value = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z)));
      applyEditorAppearance({ fontSize: fontSize.value, zoom: zoom.value });
    } catch (err) {
      console.warn('外观设置读取失败，使用默认值', asKpError(err).message);
    }
    const autosave = await readAutosaveDelay();
    if (autosave !== null) autosaveDelayMs.value = autosave;
  }

  function setTheme(next: ThemeMode): void {
    theme.value = next;
    applyTheme(next);
    persistTheme(next);
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
    autosaveDelayMs.value = next;
    persistAutosaveDelay(next);
  }

  return {
    isOpen, theme, fontSize, zoom, autosaveDelayMs, showHidden,
    configureVaultScope, loadVaultScope, setShowHidden,
    open, close, init, setTheme, toggleTheme, setFontSize, setZoom, setAutosaveDelay,
  };
});
