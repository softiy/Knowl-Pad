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
