import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, sep } from 'node:path';
import path from 'node:path';
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

  it('已发事件名与 Rust 源码**双向一致**（多一个少一个都算漂移）', () => {
    // 审计 T-05：此前这里**硬编码** 7 个名字，而 Rust 侧实际 emit 了 8 个 —— 于是
    // "把漏掉的那个补上"会让用例变红：测试把**已知漂移**锁死了，变成了漂移的保护伞。
    // 现在改为扫描 src-tauri 的生产源码，做**集合相等**比对（缺一个、多一个都红）。
    const root = path.resolve(__dirname, '../..');
    const rustNames = new Set<string>();
    const walk = (dir: string): void => {
      for (const name of readdirSync(dir)) {
        const p = join(dir, name);
        if (statSync(p).isDirectory()) {
          walk(p);
        } else if (name.endsWith('.rs') && !name.includes('_tests') && !p.includes(`${sep}tests${sep}`)) {
          for (const m of readFileSync(p, 'utf8').matchAll(/"(kp:\/\/[^"]+)"/g)) {
            rustNames.add(m[1] as string);
          }
        }
      }
    };
    walk(join(root, 'src-tauri', 'src'));
    expect(rustNames.size).toBeGreaterThan(5); // 防止"扫了个空集合"
    expect([...EMITTED_EVENT_NAMES].sort()).toEqual([...rustNames].sort());
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
