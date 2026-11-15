/**
 * 偏好写入的**防抖批量提交**（PRD FR-SET-03 / PERF-06：500ms；勘误 **DEBT-24③**）。
 *
 * 为什么需要这一层：`preference_set` 是"批量写"接口，但调用方此前是"想存就立刻存" ——
 * 多个键在同一个交互里连续变化（或用户快速连点开关）就会产生 N 次 IPC，每次都是一次写事务。
 * PRD 要求"偏好写入采用防抖批量提交（500ms）"，于是这里做一个**进程内队列**：
 * 同一窗口内的所有键合并成一次 `preference_set`（后端在**一个事务**里写完，见
 * `storage/global.rs` 的 `set_preferences`）。
 *
 * 失败语义：提交失败时把这一批条目**放回队列**（新值优先），等下一次调度或显式
 * `flushPreferences()` 重试；错误原因留在 `preferenceFlushError()` 里供界面/用例观察 ——
 * 不静默吞掉（R-15 / ERR-01）。
 *
 * 落盘时机：除 500ms 窗口外，窗口卸载/页面隐藏前由 `installPreferenceFlushOnUnload()` 强制提交
 * （否则用户改完设置立刻关窗，队列会随进程一起消失 —— 真机 S2/8.6 的另一半根因）。
 */
import { preferenceSet } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';

/** 防抖窗口（PRD FR-SET-03 的 500ms；PERF-06 同口径）。 */
export const PREFERENCE_FLUSH_MS = 500;

/** 待提交的键值（同一个键在窗口内被改多次时，后写的值覆盖先写的）。 */
const pending = new Map<string, unknown>();
let timer: ReturnType<typeof setTimeout> | null = null;
let lastError: string | null = null;

/** 队列里还有几个键（用例与调试用；不含已提交的）。 */
export function pendingPreferenceCount(): number {
  return pending.size;
}

/** 最近一次批量提交失败的原因；成功后清空。 */
export function preferenceFlushError(): string | null {
  return lastError;
}

/**
 * 排入一个窗口（500ms 后合并提交）。
 *
 * 每次调用都会**重置**定时器：连点开关时只在用户停下 500ms 后写一次。
 */
export function schedulePreferenceSet(entries: Record<string, unknown>): void {
  for (const [key, value] of Object.entries(entries)) pending.set(key, value);
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    void flushPreferences();
  }, PREFERENCE_FLUSH_MS);
}

/**
 * 立即提交队列（不等待防抖窗口）。
 *
 * 供「测试」与「需要立刻落盘的时机」使用（例如窗口关闭前）；空队列是成功的空操作。
 */
export async function flushPreferences(): Promise<void> {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
  if (pending.size === 0) return;
  const batch: Record<string, unknown> = {};
  for (const [key, value] of pending) batch[key] = value;
  pending.clear();
  try {
    await preferenceSet(batch);
    lastError = null;
  } catch (err) {
    // 放回队列：窗口内被更新的键以**新值**优先（不能拿旧值覆盖用户刚改的）
    for (const [key, value] of Object.entries(batch)) {
      if (!pending.has(key)) pending.set(key, value);
    }
    lastError = asKpError(err).message;
  }
}

/** 卸载/隐藏钩子是否已注册（重复 install 只注册一次，避免同一事件多份监听）。 */
let unloadHooksInstalled = false;

/**
 * 注册「窗口卸载前 / 页面隐藏时」立即提交防抖队列（S2 的另一半根因）。
 *
 * 为什么必须有：偏好写入是 500ms 防抖批量提交（FR-SET-03）。用户改完设置立刻关窗时定时器还没到点，
 * 队列随进程退出一起消失 —— 重启后看起来就是"设置没保存、回到默认值"（真机 8.6/S2）。
 * `beforeunload` 覆盖关窗路径；`visibilitychange → hidden` 覆盖最小化/切后台后被系统回收的路径。
 *
 * 为什么由启动编排显式调用、而不是模块加载时自动注册：单测与 SSR 会 import 本模块，
 * 自动挂全局监听会污染用例（jsdom 也没有真实卸载语义）；bootstrap 调用一次即可。
 *
 * @returns 注销函数（仅测试 / 热更新需要；正式启动不注销）。
 */
export function installPreferenceFlushOnUnload(): () => void {
  if (typeof window === 'undefined' || typeof document === 'undefined') return () => undefined;
  if (unloadHooksInstalled) return () => undefined;
  unloadHooksInstalled = true;
  // beforeunload 里不能 await；IPC 发出即可，Tauri 侧会在进程退出前尽量落盘。
  const flush = (): void => {
    void flushPreferences();
  };
  const onVisibilityChange = (): void => {
    if (document.visibilityState === 'hidden') flush();
  };
  window.addEventListener('beforeunload', flush);
  document.addEventListener('visibilitychange', onVisibilityChange);
  return () => {
    window.removeEventListener('beforeunload', flush);
    document.removeEventListener('visibilitychange', onVisibilityChange);
    unloadHooksInstalled = false;
  };
}
