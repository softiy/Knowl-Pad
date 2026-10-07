import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * IPC 事件的**唯一出口**（与 `client.ts` 之于 invoke 同理，PRD FE-04）。
 *
 * 载荷字段一律以 **PRD §5.4 的事件表为准**（勘误 D-22/D-23）：不得在此新增 PRD 未定义的字段。
 * 独立审查（2026-10-06）指出 M3 此前**前端没有任何事件监听**，状态栏用的是"invoke 失败即索引失败"的近似口径 ——
 * 本文件与 StatusBar 的改造即为修复该项。
 */

/** `kp://index/progress`（节流 ≥100ms）。 */
export interface IndexProgress {
  phase: string;
  done: number;
  total: number;
  currentFile: string | null;
}

/** `kp://index/completed` 的 stats 字段。 */
export interface IndexStatsPayload {
  indexed: number;
  skipped: number;
  cancelled: boolean;
}

/** `kp://index/completed`。 */
export interface IndexCompleted {
  stats: IndexStatsPayload;
  durationMs: number;
}

/** `kp://index/failed`（**无 detail 字段**：以 PRD 为准）。 */
export interface IndexFailed {
  code: string;
  message: string;
  failedFiles: string[];
}

/** `kp://fs/created`。 */
export interface FsCreated {
  relPath: string;
  kind: string;
}

/** `kp://fs/modified`。 */
export interface FsModified {
  relPath: string;
  mtimeMs: number;
}

/** `kp://fs/removed`。 */
export interface FsRemoved {
  relPath: string;
}

/** `kp://fs/renamed`。 */
export interface FsRenamed {
  from: string;
  to: string;
}

/** 目前 Rust 侧真正会 emit 的事件名（与 `src-tauri` 的 emit 调用一一对应）。 */
export const EMITTED_EVENT_NAMES = [
  'kp://index/progress',
  'kp://index/completed',
  'kp://index/failed',
  'kp://fs/created',
  'kp://fs/modified',
  'kp://fs/removed',
  'kp://fs/renamed',
] as const;

export type EmittedEventName = (typeof EMITTED_EVENT_NAMES)[number];

/** 薄封装：只做「事件名 + 载荷类型」的绑定，不做任何业务判断。 */
function on<T>(name: EmittedEventName, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (event) => handler(event.payload));
}

export const onIndexProgress = (h: (p: IndexProgress) => void) => on<IndexProgress>('kp://index/progress', h);
export const onIndexCompleted = (h: (p: IndexCompleted) => void) => on<IndexCompleted>('kp://index/completed', h);
export const onIndexFailed = (h: (p: IndexFailed) => void) => on<IndexFailed>('kp://index/failed', h);
export const onFsCreated = (h: (p: FsCreated) => void) => on<FsCreated>('kp://fs/created', h);
export const onFsModified = (h: (p: FsModified) => void) => on<FsModified>('kp://fs/modified', h);
export const onFsRemoved = (h: (p: FsRemoved) => void) => on<FsRemoved>('kp://fs/removed', h);
export const onFsRenamed = (h: (p: FsRenamed) => void) => on<FsRenamed>('kp://fs/renamed', h);
