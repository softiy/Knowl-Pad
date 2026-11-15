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
  /** onIndexCompleted 注册的处理器：用于模拟 completed 事件（刷新链路的回归）。 */
  completedHandler: null as null | (() => void),
}));

// 只替换这四个命令：组件不需要同模块的其它导出（类型在编译期擦除），
// 因此不必 importActual —— 那会触发项目的 @typescript-eslint/consistent-type-imports。
vi.mock('@core/ipc/commands', () => ({
  indexStatus: hoisted.indexStatus,
  indexStats: hoisted.indexStats,
  indexRebuild: hoisted.indexRebuild,
  indexCancel: hoisted.indexCancel,
}));

vi.mock('@core/ipc/events', () => ({
  onIndexProgress: vi.fn(async () => () => {}),
  onIndexCompleted: vi.fn(async (handler: () => void) => {
    hoisted.completedHandler = handler;
    return () => {};
  }),
  onIndexCancelled: vi.fn(async () => () => {}),
  onIndexFailed: vi.fn(async () => () => {}),
}));

import IndexBadge from '@/app/layout/IndexBadge.vue';

const VAULT = { root: 'C:/vaults/notes', displayName: '笔记', vaultId: 1, caseInsensitiveFs: true };
/** 就绪状态的基准返回；警告用例在此基础上加 warnings。 */
const READY_STATUS = {
  indexDir: '.knowlpad',
  indexDb: 'index.db',
  ready: true,
  schemaVersion: 1,
  signaturePrefix: 'abc',
  builtAt: 1,
  warnings: [] as string[],
};

describe('索引徽标：状态、重建与取消', () => {
  beforeEach(() => {
    hoisted.completedHandler = null;
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
    // 用对象属性而非 let 变量：赋值发生在回调里，TS 会把 let 变量窄化成 null，
    // 于是 release?.() 报 TS2349 —— CI 的 pnpm run build 抓到，本地 vue-tsc --noEmit 没抓到。
    const release: { fn: (() => void) | null } = { fn: null };
    hoisted.indexRebuild.mockImplementation(
      () =>
        new Promise((resolve) => {
          release.fn = () =>
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
    release.fn?.();
    await flushPromises();
  });

  it('没有 Vault 时不发任何需要 Vault 的命令，并显示「未打开」（DEBT-44）', async () => {
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    // 前置：有 Vault 时正常拉一次（统计进悬停详情）
    expect(hoisted.indexStatus).toHaveBeenCalledTimes(1);
    expect(hoisted.indexStats).toHaveBeenCalledTimes(1);

    hoisted.indexStatus.mockClear();
    hoisted.indexStats.mockClear();
    // 模拟「切换知识库」把 current 置空：组件常驻，watch 会触发
    await w.setProps({ vault: null });
    await flushPromises();

    // 关键断言：不再发 index_stats（否则后端必然回 E_VAULT_NOT_OPEN，并在日志里记一条假失败）
    expect(hoisted.indexStats).not.toHaveBeenCalled();
    expect(hoisted.indexStatus).not.toHaveBeenCalled();
    const el = w.get('[data-testid="status-index"]');
    expect(el.text()).toContain('未打开');
    // 不许把"什么都没做"说成"在索引"（DEBT-44 的另一面：用户可见的假状态）
    expect(el.text()).not.toContain('索引中');
    // 上一个 Vault 的统计不得残留
    expect(el.attributes('title') ?? '').toBe('');
  });

  it('有警告时显示「⚠ N 条」并把含文件名的明细放进悬停 title（SEC-03 / 勘误 D-46）', async () => {
    hoisted.indexStatus.mockResolvedValue({
      ...READY_STATUS,
      warnings: ['skip-timeout:超长单行.md', 'skip-oversize:巨大笔记.md'],
    });
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    const badge = w.get('[data-testid="index-warnings"]');
    expect(badge.text()).toContain('⚠ 2 条索引警告');
    const title = badge.attributes('title') ?? '';
    expect(title).toContain('skip-timeout:超长单行.md');
    expect(title).toContain('skip-oversize:巨大笔记.md');
  });

  it('明细最多列前 5 条，其余只报条数（状态栏不被长清单撑破）', async () => {
    const many = Array.from({ length: 8 }, (_, i) => `skip-timeout:笔记-${i}.md`);
    hoisted.indexStatus.mockResolvedValue({ ...READY_STATUS, warnings: many });
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    const title = w.get('[data-testid="index-warnings"]').attributes('title') ?? '';
    expect(title).toContain('笔记-4.md');
    expect(title).not.toContain('笔记-5.md');
    expect(title).toContain('另有 3 条');
  });

  it('无警告时不渲染警告徽标（不能凭空制造告警）', async () => {
    hoisted.indexStatus.mockResolvedValue({ ...READY_STATUS, warnings: [] });
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    expect(w.find('[data-testid="index-warnings"]').exists()).toBe(false);
  });

  it('completed 事件后 refresh 能拿到新警告（T1：完成通知之后警告仍可见）', async () => {
    hoisted.indexStatus.mockResolvedValue({ ...READY_STATUS, warnings: [] });
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    expect(w.find('[data-testid="index-warnings"]').exists()).toBe(false);

    hoisted.indexStatus.mockResolvedValue({
      ...READY_STATUS,
      warnings: ['skip-timeout:打开 Vault 时被跳过的长笔记.md'],
    });
    hoisted.completedHandler?.();
    await flushPromises();
    expect(w.get('[data-testid="index-warnings"]').text()).toContain('1 条索引警告');
  });

  it('手动重建返回的 warnings 在按钮旁可见（不静默，橙色告警）', async () => {
    hoisted.indexStatus.mockResolvedValue({ ...READY_STATUS, warnings: [] });
    hoisted.indexRebuild.mockResolvedValue({
      indexed: 3,
      skipped: 0,
      durationMs: 9,
      cancelled: false,
      warnings: ['skip-timeout:重建时跳过的长笔记.md'],
    });
    const w = mount(IndexBadge, { props: { vault: VAULT } });
    await flushPromises();
    await w.get('[data-testid="index-rebuild"]').trigger('click');
    await flushPromises();
    const badge = w.get('[data-testid="index-rebuild-warnings"]');
    expect(badge.text()).toContain('1 条警告');
    expect(badge.attributes('title')).toContain('重建时跳过的长笔记.md');
    expect(badge.classes()).toContain('kp-index__warn');
  });
});
