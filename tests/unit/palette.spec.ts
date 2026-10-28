import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import {
  COMMANDS,
  EDITOR_REASON,
  disabledReason,
  isEnabled,
  registerPaletteShortcut,
  searchCommands,
  usePaletteStore,
  type PaletteActions,
  type PaletteContext,
} from '@features/palette';
import CommandPalette from '@features/palette/components/CommandPalette.vue';

const ctx = (over: Partial<PaletteContext> = {}): PaletteContext => ({
  hasActiveNote: false,
  hasVault: true,
  mode: 'edit',
  includeHidden: false,
  ...over,
});

function stubActions(over: Partial<PaletteActions> = {}): PaletteActions {
  return {
    newNote: vi.fn(),
    quickOpen: vi.fn(),
    search: vi.fn(),
    tags: vi.fn(),
    saveNote: vi.fn(),
    saveAll: vi.fn(),
    closeNote: vi.fn(),
    toggleMode: vi.fn(),
    switchVault: vi.fn(),
    refreshTree: vi.fn(),
    toggleHidden: vi.fn(),
    ...over,
  };
}

describe('命令注册表（FR-PALETTE-02/03/05）', () => {
  it('命令 id 唯一、字段完整（名称/图标/关键词/执行体）', () => {
    const ids = COMMANDS.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const c of COMMANDS) {
      expect(c.name.length).toBeGreaterThan(0);
      expect(c.icon.length).toBeGreaterThan(0);
      expect(c.keywords.length).toBeGreaterThan(0);
      expect(typeof c.run).toBe('function');
    }
    expect(COMMANDS.length).toBeGreaterThanOrEqual(10);
  });

  it('拼音首字母命中（内置静态表，不引第三方拼音库）：xj ⇒ 新建笔记，ksdk ⇒ 快速打开', () => {
    expect(searchCommands('xj', COMMANDS)[0]?.name).toBe('新建笔记');
    expect(searchCommands('ksdk', COMMANDS)[0]?.name).toBe('快速打开');
  });

  it('英文/中文子串都能命中；大小写不敏感', () => {
    expect(searchCommands('SEARCH', COMMANDS)[0]?.name).toBe('全文搜索');
    expect(searchCommands('保存', COMMANDS).map((c) => c.name)).toContain('保存当前笔记');
  });

  it('空查询按注册顺序返回前 N 条', () => {
    expect(searchCommands('', COMMANDS)[0]?.id).toBe(COMMANDS[0]?.id);
    expect(searchCommands('', COMMANDS).length).toBeLessThanOrEqual(12);
  });

  it('上下文可用性：无打开笔记 ⇒ 编辑器命令置灰并给出原因（AC-PALETTE-02 的机制面）', () => {
    const save = COMMANDS.find((c) => c.id === 'palette.save-note')!;
    expect(isEnabled(save, ctx({ hasActiveNote: false }))).toBe(false);
    expect(disabledReason(save, ctx({ hasActiveNote: false }))).toBe(EDITOR_REASON);
    expect(isEnabled(save, ctx({ hasActiveNote: true }))).toBe(true);
    const refresh = COMMANDS.find((c) => c.id === 'palette.refresh-tree')!;
    expect(isEnabled(refresh, ctx({ hasVault: false }))).toBe(false);
  });

  it('**证据**：图谱视图在本里程碑不存在 ⇒ 不注册「打开知识图谱」（AC-PALETTE-01 的示例命令无法命中）', () => {
    expect(COMMANDS.some((c) => c.name.includes('图谱'))).toBe(false);
    expect(searchCommands('graph', COMMANDS)).toEqual([]);
  });
});

describe('性能（FR-PALETTE-10：本地模糊匹配 < 50ms）', () => {
  it('11 条命令 × 200 次查询：单次最坏与平均都远小于 50ms（机器：本机 Windows / Node 24）', () => {
    const queries = ['baocun', 'xj', 'search', 'qhv', 'hidden', '刷'];
    const durations: number[] = [];
    for (let i = 0; i < 200; i += 1) {
      const q = queries[i % queries.length] as string;
      const t0 = performance.now();
      searchCommands(q, COMMANDS);
      durations.push(performance.now() - t0);
    }
    const avg = durations.reduce((a, b) => a + b, 0) / durations.length;
    const max = Math.max(...durations);
    // 留足 CI 噪声余量：判据是 < 50ms（PRD），这里断言平均 < 5ms、最坏 < 50ms
    expect(avg).toBeLessThan(5);
    expect(max).toBeLessThan(50);
  });
});

describe('命令面板状态机（FR-PALETTE-06/07）', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('打开 ⇒ 重置查询；执行成功 ⇒ 关闭并调用执行体', async () => {
    const actions = stubActions();
    const store = usePaletteStore();
    store.configure(actions);
    store.open(ctx({ hasActiveNote: true }));
    store.setQuery('baocun');
    expect(store.results[0]?.id).toBe('palette.save-note');
    const ok = await store.execute();
    expect(ok).toBe(true);
    expect(actions.saveNote).toHaveBeenCalledTimes(1);
    expect(store.isOpen).toBe(false);
  });

  it('执行失败 ⇒ 面板保持打开并显示错误（不静默失败）', async () => {
    const actions = stubActions({ saveAll: vi.fn().mockRejectedValue(new Error('磁盘已满')) });
    const store = usePaletteStore();
    store.configure(actions);
    store.open();
    store.setQuery('baocunquanbu');
    const ok = await store.execute();
    expect(ok).toBe(false);
    expect(store.isOpen).toBe(true);
    expect(store.error).toContain('磁盘已满');
  });

  it('不可用命令不执行，只提示原因', async () => {
    const actions = stubActions();
    const store = usePaletteStore();
    store.configure(actions);
    store.open(ctx({ hasActiveNote: false }));
    store.setQuery('save');
    const idx = store.results.findIndex((c) => c.id === 'palette.save-note');
    const ok = await store.execute(idx);
    expect(ok).toBe(false);
    expect(actions.saveNote).not.toHaveBeenCalled();
    expect(store.error).toBe(EDITOR_REASON);
  });

  it('上下移动是环形的', () => {
    const store = usePaletteStore();
    store.open();
    store.move(-1);
    expect(store.activeIndex).toBe(store.results.length - 1);
    store.move(1);
    expect(store.activeIndex).toBe(0);
  });
});

describe('CommandPalette 组件（键盘：↑↓/Enter/Esc）', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('↑↓ 改变选中、Esc 关闭、Enter 执行并关闭', async () => {
    const actions = stubActions();
    const store = usePaletteStore();
    store.configure(actions);
    store.open(ctx({ hasActiveNote: true }));
    const wrapper = mount(CommandPalette);
    const input = wrapper.get('[data-testid="palette-input"]');
    await input.trigger('keydown', { key: 'ArrowDown' });
    expect(store.activeIndex).toBe(1);
    await input.trigger('keydown', { key: 'ArrowUp' });
    expect(store.activeIndex).toBe(0);
    await input.trigger('keydown', { key: 'Enter' });
    await new Promise((r) => setTimeout(r, 0));
    expect(store.isOpen).toBe(false);
    expect(Object.values(actions).some((fn) => (fn as ReturnType<typeof vi.fn>).mock.calls.length > 0)).toBe(true);
  });

  it('Esc 关闭；失焦（点背景）也关闭', async () => {
    const store = usePaletteStore();
    store.configure(stubActions());
    store.open();
    const wrapper = mount(CommandPalette);
    await wrapper.get('[data-testid="palette-input"]').trigger('keydown', { key: 'Escape' });
    expect(store.isOpen).toBe(false);
    store.open();
    await wrapper.vm.$nextTick();
    await wrapper.get('[data-testid="palette"]').trigger('mousedown');
    expect(store.isOpen).toBe(false);
  });

  it('不可用命令置灰并显示原因', async () => {
    const store = usePaletteStore();
    store.configure(stubActions());
    store.open(ctx({ hasActiveNote: false }));
    const wrapper = mount(CommandPalette);
    const item = wrapper.get('[data-testid="palette-item-palette.save-note"]');
    expect(item.classes()).toContain('kp-palette__item--disabled');
    expect(item.text()).toContain(EDITOR_REASON);
  });
});

describe('Ctrl/Cmd+P 绑定（FR-PALETTE-01）', () => {
  it('Mod+P 打开、Escape 关闭；注销后不再响应', () => {
    const open = vi.fn();
    const close = vi.fn();
    const unbind = registerPaletteShortcut(window, { open, close });
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', ctrlKey: true }));
    expect(open).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(close).toHaveBeenCalledTimes(1);
    unbind();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', ctrlKey: true }));
    expect(open).toHaveBeenCalledTimes(1);
  });
});
