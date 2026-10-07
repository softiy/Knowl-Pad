import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  }),
}));

import { EMITTED_EVENT_NAMES, onFsModified, onIndexFailed, onIndexProgress } from '@core/ipc/events';

describe('IPC 事件层（FR-VAULT-09/11 的真实信号）', () => {
  beforeEach(() => {
    listeners.clear();
  });

  it('已发事件名与 Rust 侧的 7 个 emit 一致（改动必须同步两端）', () => {
    expect([...EMITTED_EVENT_NAMES]).toEqual([
      'kp://index/progress',
      'kp://index/completed',
      'kp://index/failed',
      'kp://fs/created',
      'kp://fs/modified',
      'kp://fs/removed',
      'kp://fs/renamed',
    ]);
  });

  it('onIndexProgress：监听 kp://index/progress 并原样透传载荷', async () => {
    const seen: unknown[] = [];
    await onIndexProgress((p) => seen.push(p));
    expect(listeners.has('kp://index/progress')).toBe(true);
    listeners.get('kp://index/progress')?.({ payload: { phase: 'full', done: 3, total: 10, currentFile: null } });
    expect(seen).toEqual([{ phase: 'full', done: 3, total: 10, currentFile: null }]);
  });

  it('onIndexFailed：载荷只含 code/message/failedFiles（PRD 无 detail 字段）', async () => {
    const seen: unknown[] = [];
    await onIndexFailed((p) => seen.push(p));
    listeners.get('kp://index/failed')?.({ payload: { code: 'E_INDEX', message: '索引失败', failedFiles: ['a.md'] } });
    expect(seen).toEqual([{ code: 'E_INDEX', message: '索引失败', failedFiles: ['a.md'] }]);
  });

  it('onFsModified：透传真实 mtime（独立审查指出此前恒为 0）', async () => {
    const seen: { relPath: string; mtimeMs: number }[] = [];
    await onFsModified((p) => seen.push(p));
    listeners.get('kp://fs/modified')?.({ payload: { relPath: 'a.md', mtimeMs: 1730000000000 } });
    expect(seen[0]?.mtimeMs).toBe(1730000000000);
  });
});
