/**
 * 编辑器设置项的持久化（PRD §4.11.3 里作用域为「全局」的那几项；FR-EDITOR-30 / FR-SET-03）。
 *
 * 为什么单独一个模块：① 设置读写是"读库 + 夹取 + 写库"的纯逻辑，与编辑缓冲区的状态机无关；
 * ② 塞进 `editor.ts` 的 setup 会让它撞 `max-lines-per-function`（CODE-11 同源的硬约束，不允许豁免）。
 */
import { preferenceGet } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import { schedulePreferenceSet } from '@core/ipc/preferences';

/** 自动保存防抖默认值（FR-EDITOR-30：默认 1s，可配 0.5–5s）。 */
export const AUTOSAVE_DEFAULT_MS = 1000;
export const AUTOSAVE_MIN_MS = 500;
export const AUTOSAVE_MAX_MS = 5000;

/**
 * 自动保存防抖时长的**全局偏好键**（PRD §4.11.3「自动保存防抖时长 | 1000ms | **全局**」；FR-EDITOR-30）。
 *
 * 技术方案 §13.2 的 **DEBT-30**：此值此前只在内存 `ref` 里，"重启后回到默认值"。
 * 现在落全局库的 `preference`（跨 Vault 生效，与 §4.11.3 的作用域一致），写入走 500ms 防抖批量提交（FR-SET-03）。
 */
export const PREF_AUTOSAVE_DELAY_MS = 'editor.autosaveDelayMs';

/** 夹取到 FR-EDITOR-30 的范围（0.5–5s）并取整；写入、读取与恢复共用同一口径。 */
export function clampAutosaveDelay(ms: number): number {
  return Math.min(AUTOSAVE_MAX_MS, Math.max(AUTOSAVE_MIN_MS, Math.round(ms)));
}

/**
 * 读取持久化的防抖时长（DEBT-30）。
 *
 * 返回值语义（三种情况分开，别混）：
 * - 有值 ⇒ 夹取后的毫秒数；
 * - **没有**这个键 ⇒ `null`（调用方保持默认，不覆盖当前值）；
 * - 键存在但值非法（旧版本写坏 / 手改过库）⇒ {@link AUTOSAVE_DEFAULT_MS}（不让坏值影响行为）；
 * - 读取失败（IPC 抖动）⇒ `null` + 控制台提示 —— 不能因为一次读失败就把用户刚设的值改掉（R-15 不静默）。
 */
export async function readAutosaveDelay(): Promise<number | null> {
  try {
    const stored = await preferenceGet([PREF_AUTOSAVE_DELAY_MS]);
    const raw = stored.values[PREF_AUTOSAVE_DELAY_MS];
    if (typeof raw === 'number' && Number.isFinite(raw)) return clampAutosaveDelay(raw);
    return raw === undefined ? null : AUTOSAVE_DEFAULT_MS;
  } catch (err) {
    console.warn('自动保存防抖时长读取失败，使用默认值', asKpError(err).message);
    return null;
  }
}

/** 持久化防抖时长：走 500ms 防抖批量提交（FR-SET-03 / PERF-06），连点只会产生一次写。 */
export function persistAutosaveDelay(ms: number): void {
  schedulePreferenceSet({ [PREF_AUTOSAVE_DELAY_MS]: ms });
}
