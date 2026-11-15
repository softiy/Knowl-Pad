import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import {
  clampSidebarWidth, NARROW_BREAKPOINT, SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH,
  STATE_KEY_RIGHT_PANEL, STATE_KEY_SIDEBAR_COLLAPSED, STATE_KEY_SIDEBAR_WIDTH, useLayoutStore,
  type VaultStateIO,
} from '@features/layout';
import Ribbon from '@features/layout/components/Ribbon.vue';
import { beginSidebarResize, watchViewport } from '@features/layout/viewport';
import { COMMANDS, searchCommands } from '@features/palette';

vi.mock('@core/ipc/commands', () => ({ vaultStateGet: vi.fn(), vaultStateSet: vi.fn() }));
vi.mock('@core/ipc/preferences', () => ({ schedulePreferenceSet: vi.fn() }));

/** 按 Vault 分桶的假 vault_state：直接证明「切 Vault 不串」。 */
function fakeIO(initial: Record<string, unknown> = {}): VaultStateIO & { reads: string[][]; writes: Record<string, unknown>[] } {
  const values = { ...initial };
  const reads: string[][] = [];
  const writes: Record<string, unknown>[] = [];
  return {
    reads,
    writes,
    read: async (keys) => {
      reads.push(keys);
      return Object.fromEntries(keys.map((key) => [key, values[key]]));
    },
    write: async (key, value) => {
      writes.push({ [key]: value });
      values[key] = value;
    },
  };
}

describe('布局状态（FR-LAYOUT-01/03/04/05）', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('宽度夹取 240–520，非法值回默认（默认 280 不变）', () => {
    expect(SIDEBAR_MIN_WIDTH).toBe(240);
    expect(SIDEBAR_MAX_WIDTH).toBe(520);
    expect(SIDEBAR_DEFAULT_WIDTH).toBe(280);
    expect(clampSidebarWidth(10)).toBe(240);
    expect(clampSidebarWidth(999)).toBe(520);
    expect(clampSidebarWidth(Number.NaN)).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it('右栏默认关闭（用户反馈：反链默认打开会挤占编辑区）', () => {
    const store = useLayoutStore();
    expect(store.rightPanel).toBe('hidden');
  });

  it('折叠/展开写 vault_state（键 ui.sidebarCollapsed），不是 preference', async () => {
    const store = useLayoutStore();
    const io = fakeIO();
    store.configureVaultState(io);
    await store.toggleSidebar();
    expect(store.sidebarCollapsed).toBe(true);
    expect(io.writes).toEqual([{ [STATE_KEY_SIDEBAR_COLLAPSED]: true }]);
    await store.toggleSidebar();
    expect(io.writes.at(-1)).toEqual({ [STATE_KEY_SIDEBAR_COLLAPSED]: false });
  });

  it('拖拽调宽后落盘，且被夹取', async () => {
    const store = useLayoutStore();
    const io = fakeIO();
    store.configureVaultState(io);
    await store.setSidebarWidth(9999);
    expect(store.sidebarWidth).toBe(520);
    expect(io.writes.at(-1)).toEqual({ [STATE_KEY_SIDEBAR_WIDTH]: 520 });
  });

  it('恢复：一次批量读取（同一 Vault 快照）读回折叠/宽度/右侧面板（FR-LAYOUT-05 的地基）', async () => {
    const store = useLayoutStore();
    const io = fakeIO({
      [STATE_KEY_SIDEBAR_COLLAPSED]: true,
      [STATE_KEY_SIDEBAR_WIDTH]: 420,
      [STATE_KEY_RIGHT_PANEL]: 'links',
    });
    store.configureVaultState(io);
    await store.load();
    // 三个键一次读：分开读会在切库瞬间混库（9.6）
    expect(io.reads).toEqual([[STATE_KEY_SIDEBAR_COLLAPSED, STATE_KEY_SIDEBAR_WIDTH, STATE_KEY_RIGHT_PANEL]]);
    expect(store.sidebarCollapsed).toBe(true);
    expect(store.sidebarWidth).toBe(420);
    expect(store.rightPanel).toBe('links');
  });

  it('切库竞态：旧 load 的慢响应不得覆盖新 load（9.6）', async () => {
    const store = useLayoutStore();
    const pending: ((values: Record<string, unknown>) => void)[] = [];
    store.configureVaultState({
      read: () => new Promise((resolve) => { pending.push(resolve); }),
      write: async () => undefined,
    });
    const stale = store.load(); // 旧库（慢）
    const fresh = store.load(); // 新库（快）
    pending[1]({ [STATE_KEY_SIDEBAR_WIDTH]: 400 });
    pending[0]({ [STATE_KEY_SIDEBAR_WIDTH]: 500, [STATE_KEY_SIDEBAR_COLLAPSED]: true });
    await Promise.all([stale, fresh]);
    expect(store.sidebarWidth).toBe(400);
    expect(store.sidebarCollapsed).toBe(false);
  });

  it('reset()：切库瞬间回默认且**不写盘**（新库的值由 load 决定）', async () => {
    const store = useLayoutStore();
    const io = fakeIO({ [STATE_KEY_SIDEBAR_WIDTH]: 500, [STATE_KEY_SIDEBAR_COLLAPSED]: true, [STATE_KEY_RIGHT_PANEL]: 'links' });
    store.configureVaultState(io);
    await store.load();
    store.reset();
    expect(store.sidebarCollapsed).toBe(false);
    expect(store.sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(store.rightPanel).toBe('hidden');
    expect(io.writes).toEqual([]);
  });

  it('右栏显式设置并落盘；至多一次写入；窄屏阈值只影响临时态不写盘（FR-LAYOUT-04）', async () => {
    const store = useLayoutStore();
    const io = fakeIO();
    store.configureVaultState(io);
    await store.setRightPanel('links');
    expect(store.rightPanel).toBe('links');
    expect(io.writes.at(-1)).toEqual({ [STATE_KEY_RIGHT_PANEL]: 'links' });
    await store.setRightPanel('links'); // 幂等：同一值不重复落盘
    expect(io.writes).toHaveLength(1);
    await store.toggleRightPanel();
    expect(store.rightPanel).toBe('hidden');

    const before = io.writes.length;
    store.applyViewport(NARROW_BREAKPOINT - 1);
    expect(store.narrow).toBe(true);
    expect(store.effectiveCollapsed).toBe(true);
    expect(io.writes).toHaveLength(before); // narrow 只是临时态
    store.applyViewport(NARROW_BREAKPOINT + 100);
    expect(store.narrow).toBe(false);
    expect(store.effectiveCollapsed).toBe(false); // 放大后恢复用户显式设置
  });
});

describe('Ribbon（FR-LAYOUT-02：只放真实入口）', () => {
  it('包含文件树/搜索/标签/右侧面板/设置 5 个按钮，且不造图谱/回收站', () => {
    const wrapper = mount(Ribbon, { props: { collapsed: false, rightPanelVisible: true } });
    for (const id of ['ribbon-files', 'ribbon-search', 'ribbon-tags', 'ribbon-right', 'ribbon-settings']) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists()).toBe(true);
    }
    expect(wrapper.find('[data-testid="ribbon-graph"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="ribbon-trash"]').exists()).toBe(false);
  });

  it('点击发出对应事件', async () => {
    const wrapper = mount(Ribbon, { props: { collapsed: false, rightPanelVisible: true } });
    await wrapper.get('[data-testid="ribbon-files"]').trigger('click');
    await wrapper.get('[data-testid="ribbon-search"]').trigger('click');
    await wrapper.get('[data-testid="ribbon-tags"]').trigger('click');
    await wrapper.get('[data-testid="ribbon-right"]').trigger('click');
    await wrapper.get('[data-testid="ribbon-settings"]').trigger('click');
    expect(wrapper.emitted('toggleSidebar')).toHaveLength(1);
    expect(wrapper.emitted('search')).toHaveLength(1);
    expect(wrapper.emitted('tags')).toHaveLength(1);
    expect(wrapper.emitted('toggleRight')).toHaveLength(1);
    expect(wrapper.emitted('settings')).toHaveLength(1);
  });

  it('搜索/标签按钮按「已打开」高亮（开关语义的可感知状态）', () => {
    const wrapper = mount(Ribbon, {
      props: { collapsed: false, rightPanelVisible: false, searchOpen: true, tagsOpen: false },
    });
    expect(wrapper.get('[data-testid="ribbon-search"]').attributes('aria-pressed')).toBe('true');
    expect(wrapper.get('[data-testid="ribbon-tags"]').attributes('aria-pressed')).toBe('false');
    expect(wrapper.get('[data-testid="ribbon-right"]').attributes('aria-pressed')).toBe('false');
  });
});

describe('命令面板布局命令（共 15 条）', () => {
  it('「切换左侧栏」「切换右侧面板」可搜到，执行体调用注入的方法', async () => {
    expect(COMMANDS).toHaveLength(15);
    expect(searchCommands('cebianlan', COMMANDS)[0]?.name).toBe('切换左侧栏（折叠/展开）');
    expect(searchCommands('ycmb', COMMANDS)[0]?.name).toBe('切换右侧面板');
    const calls: string[] = [];
    await COMMANDS.find((c) => c.id === 'palette.toggle-sidebar')!.run({ toggleSidebar: () => { calls.push('sidebar'); } } as never);
    await COMMANDS.find((c) => c.id === 'palette.toggle-right-panel')!.run({ toggleRightPanel: () => { calls.push('right'); } } as never);
    expect(calls).toEqual(['sidebar', 'right']);
  });
});

/**
 * 9.7 的两个机制面（此前只有 CSS，没有可回归的行为）：
 * ① resize 监听必须「挂载一次、卸载注销」，且在**任意**时刻都更新窄屏状态；
 * ② 侧栏拖拽只在 pointerup 落盘一次，宽度严格夹取在 240–520。
 */
describe('窗口尺寸与侧栏拖拽（FR-LAYOUT-01/04、9.7）', () => {
  it('watchViewport：注册即量一次，resize 时回调；注销后不再回调', () => {
    const original = Object.getOwnPropertyDescriptor(window, 'innerWidth');
    Object.defineProperty(window, 'innerWidth', { value: 800, configurable: true, writable: true });
    const widths: number[] = [];
    const dispose = watchViewport((width) => widths.push(width));
    expect(widths).toEqual([800]);
    window.innerWidth = 1280;
    window.dispatchEvent(new Event('resize'));
    expect(widths).toEqual([800, 1280]);
    dispose();
    window.dispatchEvent(new Event('resize'));
    expect(widths).toEqual([800, 1280]); // 已注销：不再回调（ED-01）
    if (original === undefined) delete (window as unknown as { innerWidth?: number }).innerWidth;
    else Object.defineProperty(window, 'innerWidth', original);
  });

  it('拖拽：预览夹取在 240–520，拖拽中不提交，pointerup 只提交一次', () => {
    const previews: number[] = [];
    const commits: number[] = [];
    beginSidebarResize(
      new MouseEvent('pointerdown', { clientX: 300 }) as PointerEvent,
      280,
      (width) => previews.push(width),
      (width) => commits.push(width),
    );
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 360 }));
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 900 })); // 拖出右边界 → 夹到 520
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 0 })); // 拖出左边界 → 夹到 240
    expect(previews).toEqual([340, 520, 240]);
    expect(commits).toEqual([]); // 拖拽过程中不落盘
    window.dispatchEvent(new MouseEvent('pointerup'));
    expect(commits).toEqual([240]);
    window.dispatchEvent(new MouseEvent('pointerup')); // 重复 pointerup 不重复提交
    expect(commits).toEqual([240]);
  });

  it('没拖就松手：不提交（不产生无意义的 vault_state 写）', () => {
    const commits: number[] = [];
    beginSidebarResize(
      new MouseEvent('pointerdown', { clientX: 100 }) as PointerEvent,
      300,
      () => undefined,
      (width) => commits.push(width),
    );
    window.dispatchEvent(new MouseEvent('pointerup'));
    expect(commits).toEqual([]);
  });
});
