import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import {
  clampSidebarWidth, NARROW_BREAKPOINT, SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH,
  STATE_KEY_RIGHT_PANEL, STATE_KEY_SIDEBAR_COLLAPSED, STATE_KEY_SIDEBAR_WIDTH, useLayoutStore,
} from '@features/layout';
import Ribbon from '@features/layout/components/Ribbon.vue';
import { COMMANDS, searchCommands } from '@features/palette';

vi.mock('@core/ipc/commands', () => ({ vaultStateGet: vi.fn(), vaultStateSet: vi.fn() }));
vi.mock('@core/ipc/preferences', () => ({ schedulePreferenceSet: vi.fn() }));

describe('布局状态（FR-LAYOUT-01/03/04）', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('宽度夹取 200–600，非法值回默认', () => {
    expect(SIDEBAR_MIN_WIDTH).toBe(200);
    expect(SIDEBAR_MAX_WIDTH).toBe(600);
    expect(clampSidebarWidth(10)).toBe(200);
    expect(clampSidebarWidth(999)).toBe(600);
    expect(clampSidebarWidth(Number.NaN)).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it('折叠/展开写 vault_state（键 ui.sidebarCollapsed），不是 preference', async () => {
    const store = useLayoutStore();
    const written: Record<string, unknown> = {};
    store.configureVaultState({
      read: async () => undefined,
      write: async (key, value) => { written[key] = value; },
    });
    await store.toggleSidebar();
    expect(store.sidebarCollapsed).toBe(true);
    expect(written[STATE_KEY_SIDEBAR_COLLAPSED]).toBe(true);
    await store.toggleSidebar();
    expect(written[STATE_KEY_SIDEBAR_COLLAPSED]).toBe(false);
  });

  it('拖拽调宽后落盘，且被夹取', async () => {
    const store = useLayoutStore();
    const written: Record<string, unknown> = {};
    store.configureVaultState({ read: async () => undefined, write: async (k, v) => { written[k] = v; } });
    await store.setSidebarWidth(9999);
    expect(store.sidebarWidth).toBe(600);
    expect(written[STATE_KEY_SIDEBAR_WIDTH]).toBe(600);
  });

  it('恢复：读回折叠/宽度/右侧面板（FR-LAYOUT-05 的地基）', async () => {
    const store = useLayoutStore();
    const values: Record<string, unknown> = {
      [STATE_KEY_SIDEBAR_COLLAPSED]: true,
      [STATE_KEY_SIDEBAR_WIDTH]: 420,
      [STATE_KEY_RIGHT_PANEL]: 'hidden',
    };
    store.configureVaultState({ read: async (k) => values[k], write: async () => undefined });
    await store.load();
    expect(store.sidebarCollapsed).toBe(true);
    expect(store.sidebarWidth).toBe(420);
    expect(store.rightPanel).toBe('hidden');
  });

  it('右侧面板切换并落盘；窄屏阈值自动折叠（FR-LAYOUT-04）', async () => {
    const store = useLayoutStore();
    const written: Record<string, unknown> = {};
    store.configureVaultState({ read: async () => undefined, write: async (k, v) => { written[k] = v; } });
    await store.toggleRightPanel();
    expect(store.rightPanel).toBe('hidden');
    expect(written[STATE_KEY_RIGHT_PANEL]).toBe('hidden');

    store.applyViewport(NARROW_BREAKPOINT - 1);
    expect(store.narrow).toBe(true);
    expect(store.effectiveCollapsed).toBe(true);
    store.applyViewport(NARROW_BREAKPOINT + 100);
    expect(store.narrow).toBe(false);
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
});

describe('命令面板新增两条（共 15 条）', () => {
  it('「切换左侧栏」「切换右侧面板」可搜到，执行体调用布局 store 的注入方法', async () => {
    expect(COMMANDS).toHaveLength(15);
    expect(searchCommands('cebianlan', COMMANDS)[0]?.name).toBe('切换左侧栏（折叠/展开）');
    expect(searchCommands('ycmb', COMMANDS)[0]?.name).toBe('切换右侧面板');
    const calls: string[] = [];
    await COMMANDS.find((c) => c.id === 'palette.toggle-sidebar')!.run({ toggleSidebar: () => { calls.push('sidebar'); } } as never);
    await COMMANDS.find((c) => c.id === 'palette.toggle-right-panel')!.run({ toggleRightPanel: () => { calls.push('right'); } } as never);
    expect(calls).toEqual(['sidebar', 'right']);
  });
});
