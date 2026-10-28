/**
 * 主题与外观令牌（FR-SET-20/21/23；DEBT-18 的收口）。
 *
 * 三种模式：`light` / `dark` / `system`（FR-SET-20 明确要求「跟随系统」）。
 * 真实生效的只有 light/dark：`applyTheme` 把**解析后**的值写到 `<html data-theme>`，
 * CSS 变量随之切换；system 模式下监听 `prefers-color-scheme` 的变化。
 */
import { preferenceGet } from '@core/ipc/commands';
import { schedulePreferenceSet } from '@core/ipc/preferences';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

export const THEMES: readonly ThemeMode[] = ['light', 'dark', 'system'];
export const DEFAULT_THEME: ThemeMode = 'system';
/** 全局偏好键（PRD §4.11.3「主题 | 跟随系统 | 全局」；写入走 500ms 防抖）。 */
export const PREF_THEME = 'appearance.theme';
export const PREF_FONT_SIZE = 'appearance.editorFontSize';
export const PREF_ZOOM = 'appearance.zoomPercent';

export const FONT_SIZE_MIN = 12;
export const FONT_SIZE_MAX = 24;
export const ZOOM_MIN = 80;
export const ZOOM_MAX = 150;

/**
 * 主题令牌的**可计算镜像**（与 tokens.css 同值）。
 *
 * 为什么要在 JS 里再存一份：对比度断言必须能算（FR-SET-25 / AC-SET-03 的证据），
 * 而 jsdom 不算样式表里的变量；这里只服务测试与调试，UI 一切颜色仍以 CSS 变量为准。
 */
export const THEME_TOKENS: Readonly<Record<ResolvedTheme, Readonly<Record<string, string>>>> = {
  light: {
    bg: '#ffffff',
    fg: '#1f2328',
    muted: '#57606a',
    accent: '#1565c0',
    codeBg: '#f6f8fa',
    keyword: '#a626a4',
    string: '#3a7d32',
    comment: '#57606a',
    number: '#7a5200',
    literal: '#0550ae',
    variable: '#b3261e',
    heading: '#0b5394',
    code: '#a61b3c',
    link: '#1565c0',
    url: '#57606a',
  },
  dark: {
    bg: '#0d1117',
    fg: '#e6edf3',
    muted: '#8b949e',
    accent: '#58a6ff',
    codeBg: '#161b22',
    keyword: '#ff7b72',
    string: '#a5d6ff',
    comment: '#8b949e',
    number: '#79c0ff',
    literal: '#79c0ff',
    variable: '#ffa657',
    heading: '#79c0ff',
    code: '#ffa657',
    link: '#58a6ff',
    url: '#8b949e',
  },
};

export function tokensFor(mode: ResolvedTheme): Readonly<Record<string, string>> {
  return THEME_TOKENS[mode];
}

/** 纯函数：system 依据系统偏好，其余原样返回（便于单测）。 */
export function resolveTheme(mode: ThemeMode, prefersDark: boolean): ResolvedTheme {
  if (mode === 'system') return prefersDark ? 'dark' : 'light';
  return mode;
}

export function systemPrefersDark(): boolean {
  return typeof window !== 'undefined' && window.matchMedia !== undefined
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
    : false;
}

/** 应用主题：写 `<html data-theme>`（变量随之切换，无需重载），返回解析后的模式。 */
export function applyTheme(mode: ThemeMode): ResolvedTheme {
  const resolved = resolveTheme(mode, systemPrefersDark());
  document.documentElement.dataset.theme = resolved;
  return resolved;
}

/** 当前生效的主题（供编辑器等需要 light/dark 的适配器读取）。 */
export function currentTheme(): ResolvedTheme {
  const attr = document.documentElement.dataset.theme;
  return attr === 'dark' ? 'dark' : 'light';
}

export function isThemeMode(value: unknown): value is ThemeMode {
  return value === 'light' || value === 'dark' || value === 'system';
}

/** 读取持久化的主题（读失败/坏值 ⇒ 默认，不把坏值写回；R-15 不静默）。 */
export async function loadTheme(): Promise<ThemeMode> {
  try {
    const raw = (await preferenceGet([PREF_THEME])).values[PREF_THEME];
    return isThemeMode(raw) ? raw : DEFAULT_THEME;
  } catch (err) {
    console.warn('主题读取失败，使用默认值', err instanceof Error ? err.message : String(err));
    return DEFAULT_THEME;
  }
}

/** 持久化主题（500ms 防抖批量，FR-SET-03）。 */
export function persistTheme(mode: ThemeMode): void {
  schedulePreferenceSet({ [PREF_THEME]: mode });
}

/** 外观有关的 CSS 变量（FR-SET-22/23；只改根节点变量，布局不破版）。 */
export function applyEditorAppearance(input: { fontSize?: number; zoom?: number }): void {
  const root = document.documentElement;
  if (input.fontSize !== undefined) root.style.setProperty('--kp-editor-font-size', input.fontSize + 'px');
  if (input.zoom !== undefined) root.style.setProperty('--kp-zoom', String(input.zoom / 100));
}

/** WCAG 2.1 相对亮度（sRGB）。 */
function channel(v: number): number {
  const c = v / 255;
  return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

export function relLuminance(hex: string): number {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (m === null) throw new Error('只支持 #rrggbb：' + hex);
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG 对比度（≥ 4.5 为正文 AA；FR-SET-25 的判据）。 */
export function contrastRatio(a: string, b: string): number {
  const la = relLuminance(a);
  const lb = relLuminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}
