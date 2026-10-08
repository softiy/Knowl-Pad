import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * 索引徽标（FR-VAULT-11 状态显示、FR-VAULT-09 可取消）。
 *
 * 这三个命令（index_stats / index_rebuild / index_cancel）此前**前端零消费**：
 * 用户既看不到索引规模，也没有重建与取消的入口。本 spec 钉住这三个入口。
 */
const hoisted = vi.hoisted(() => ({
  indexStatus: vi.fn(),
  indexStats: vi.fn(),
  indexRebuild: vi.fn(),
  indexCancel: vi.fn(),
}));

vi.mock('@core/ipc/commands', async () => {
  const actual = await vi.importActual<typeof import('@core/ipc/commands')>('@core/ipc/commands');
  return {
    ...actual,
    indexStatus: hoisted.indexStatus,
    indexStats: hoisted.indexStats,
    indexRebuild: hoisted.indexRebuild,
    indexCancel: hoisted.indexCancel,
  };
});

vi.mock('@core/ipc/events', () => ({
  onIndexProgress: vi.fn(async () => () => {}),
  onIndexCompleted: vi.fn(async () => () => {}),
  onIndexFailed: vi.fn(async () => () => {}),
}));

import IndexBadge from '@/app/layout/IndexBadge.vue';

const VAULT = { root: 'C:/vaults/notes', displayName: '笔记', vaultId: 1, caseInsensitiveFs: true };

describe('索引徽标：状态、重建与取消', () => {
  beforeEach(() => {
    hoisted.indexStatus.mockReset().mockResolvedValue({
      indexDir: '.knowlpad',
      indexDb: 'index.db',
      ready: true,
      schemaVersion: 1,
      signaturePrefix: 'abc',
      builtAt: 1,
    });
    hoisted.indexStats.mockReset().mockResolvedValue({ files: 12, links: 30, tags: 4 });
    hoisted.indexRebuild.mockReset().mockResolvedValue({
      indexed: 12,
      skipped: 0,
      durationMs: 5,
      cancelled: false,
      warnings: [],
    });
    hoisted.indexCancel.mockReset().mockResolvedValue(true);
  });

  it('就绪时显示状态，并把索引规模放进悬停详情（index_stats 的真实消费方）', async () => {
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    expect(w.get('[data-testid="status-index"]').text()).toContain('就绪');
    expect(w.get('[data-testid="status-index"]').attributes('title')).toContain('文件 12');
    expect(hoisted.indexStats).toHaveBeenCalled();
  });

  it('ready=false 时显示索引中，且提供可执行的重建入口（FR-VAULT-11）', async () => {
    hoisted.indexStatus.mockResolvedValue({
      indexDir: '.knowlpad',
      indexDb: 'index.db',
      ready: false,
      schemaVersion: null,
      signaturePrefix: null,
      builtAt: null,
    });
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    expect(w.get('[data-testid="status-index"]').text()).toContain('索引中');
    await w.get('[data-testid="index-rebuild"]').trigger('click');
    await flushPromises();
    expect(hoisted.indexRebuild).toHaveBeenCalledWith(true);
    expect(hoisted.indexStatus).toHaveBeenCalledTimes(2); // 重建后刷新
  });

  it('重建进行中提供取消入口（FR-VAULT-09：大批量操作可取消）', async () => {
    let release: (() => void) | null = null;
    hoisted.indexRebuild.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () =>
            resolve({ indexed: 0, skipped: 0, durationMs: 1, cancelled: true, warnings: [] });
        }),
    );
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    await w.get('[data-testid="index-rebuild"]').trigger('click');
    await flushPromises();
    const cancel = w.get('[data-testid="index-cancel"]');
    await cancel.trigger('click');
    expect(hoisted.indexCancel).toHaveBeenCalled();
    release?.();
    await flushPromises();
  });
});
