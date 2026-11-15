import { mount, flushPromises } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// ── vault 域替身：本 spec 只关心「切换知识库」这条链路，不装真实 IPC ──
vi.mock('@features/vault', async () => {
  // 真实项目用 Pinia store（ref 会被自动解包），这里用 reactive 复现同样的语义：
  // 组件模板写的是 `vaultStore.current.displayName`，裸 ref 会得到 undefined。
  const { ref, reactive } = await import('vue');
  // 注意：vi.mock 的工厂会被提升，**不能**引用模块作用域的变量（会 ReferenceError），
  // 所以这里的字面量与文件下方的 VAULT 常量保持一致（重复是刻意的）。
  const current = ref<{
    root: string;
    displayName: string;
    vaultId: number;
    caseInsensitiveFs: boolean;
  } | null>({ root: 'C:/vaults/notes', displayName: '我的知识库', vaultId: 1, caseInsensitiveFs: true });
  const error = ref<string | null>(null);
  const store = reactive({
    current,
    error,
    init: vi.fn(async () => {}),
    closeCurrent: vi.fn(async () => {
      current.value = null;
    }),
  });
  return { useVaultStore: () => store };
});

/** 供 beforeEach 重置 mock 用（形状与 mock 工厂内的一致）。 */
const VAULT = { root: 'C:/vaults/notes', displayName: '我的知识库', vaultId: 1, caseInsensitiveFs: true };

// ── 链接域替身（布局会 new 它；切库要断言 reset 被调用）──
const linksHoisted = vi.hoisted(() => ({ reset: vi.fn(), refresh: vi.fn(async () => {}) }));
vi.mock('@features/links', async () => {
  const { reactive } = await import('vue');
  const store = reactive({
    activeRelPath: 'a.md' as string | null,
    backlinks: [] as unknown[],
    reset: linksHoisted.reset,
    refresh: linksHoisted.refresh,
  });
  return {
    LinksPanel: { name: 'LinksPanel', template: '<div data-testid="links-panel" />' },
    useLinksStore: () => store,
  };
});

// ── 编辑器域替身：切库必须 closeAll（串文件），保存流程沿用既有断言 ──
const dirtyFlag = { value: false };
const editorMock = vi.hoisted(() => ({
  buffers: [{ relPath: 'a.md', content: '', savedContent: '', error: null as string | null }],
  saveAll: vi.fn(async () => {}),
  hasUnsaved: vi.fn(() => false),
  closeAll: vi.fn(),
}));
vi.mock('@features/editor', async () => {
  return {
    EditorView: { template: '<div data-testid="editor" />' },
    useEditorStore: () => ({
      ...editorMock,
      activeRelPath: null,
      mode: 'edit',
      autosaveDelayMs: 1000,
      setAutosaveDelay: vi.fn(),
      isDirty: vi.fn(() => dirtyFlag.value),
      openNote: vi.fn(async () => {}),
      save: vi.fn(async () => {}),
      closeNote: vi.fn(),
      setMode: vi.fn(),
    }),
  };
});

// ── 搜索/标签域替身：切库必须关面板（串面板）──
const searchHoisted = vi.hoisted(() => ({ closePanel: vi.fn() }));
vi.mock('@features/search', async () => {
  const { reactive } = await import('vue');
  const store = reactive({
    panelOpen: false,
    quickOpenVisible: false,
    openPanel: vi.fn(() => { store.panelOpen = true; }),
    closePanel: searchHoisted.closePanel,
    openQuickOpen: vi.fn(() => { store.quickOpenVisible = true; }),
    closeQuickOpen: vi.fn(() => { store.quickOpenVisible = false; }),
  });
  searchHoisted.closePanel.mockImplementation(() => { store.panelOpen = false; });
  return {
    SearchPanel: { name: 'SearchPanel', template: '<div data-testid="search-panel" />' },
    QuickOpen: { name: 'QuickOpen', template: '<div data-testid="quick-open" />' },
    useSearchStore: () => store,
  };
});

const tagsHoisted = vi.hoisted(() => ({ closePanel: vi.fn() }));
vi.mock('@features/tags', async () => {
  const { reactive, defineComponent } = await import('vue');
  const store = reactive({
    panelOpen: false,
    openPanel: vi.fn(() => { store.panelOpen = true; }),
    closePanel: tagsHoisted.closePanel,
  });
  tagsHoisted.closePanel.mockImplementation(() => { store.panelOpen = false; });
  // 替身必须复刻真身的**对外 API**（`defineExpose` 的 focusFilter）：
  // 少了它，布局的"打开面板后聚焦过滤框"会在运行期炸出 unhandled rejection。
  const TagPanel = defineComponent({
    name: 'TagPanel',
    emits: ['open', 'close'],
    setup: (_p, { expose, emit }) => {
      expose({ focusFilter: vi.fn() });
      return { emit };
    },
    template:
      '<div>' +
      '<button type="button" data-testid="tag-panel-close" @click="emit(\'close\')">关闭</button>' +
      '</div>',
  });
  return { TagPanel, useTagsStore: () => store };
});

// ── 文件树/文件操作域替身：切库必须清展开态与操作提示（串目录/串提示）──
const fileTreeMock = vi.hoisted(() => {
  const store = {
    expanded: new Set<string>(),
    rows: [] as unknown[],
    error: null as string | null,
    includeHidden: true,
    restoreState: vi.fn(async () => {}),
    refresh: vi.fn(async () => {}),
    setIncludeHidden: vi.fn(async () => {}),
    // 语义与真实 file-tree store 的 reset() 一致：清展开态/行/错误，隐藏开关回默认
    reset: vi.fn(() => {
      store.expanded = new Set();
      store.rows = [];
      store.error = null;
      store.includeHidden = false;
    }),
  };
  return store;
});
const opsMock = vi.hoisted(() => {
  const store = {
    notice: '已新建 x.md' as string | null,
    openNewNote: vi.fn(),
    openNewFolder: vi.fn(),
    openRename: vi.fn(),
    openMove: vi.fn(),
    openDelete: vi.fn(),
    createFromLink: vi.fn(),
    clearNotice: vi.fn(() => { store.notice = null; }),
  };
  return store;
});
vi.mock('@features/file-tree', () => ({
  FileTree: { template: '<div />' },
  FileOpsDialog: { template: '<div />' },
  useFileTreeStore: () => fileTreeMock,
  useFileOpsStore: () => opsMock,
}));

const clearRenderCacheMock = vi.hoisted(() => vi.fn());
vi.mock('@core/markdown/renderer', () => ({ clearRenderCache: clearRenderCacheMock }));

vi.mock('@features/vault/components/VaultList.vue', () => ({ default: { template: '<div data-testid="vault-list" />' } }));
vi.mock('@core/ipc/commands', () => ({
  vaultStateGet: vi.fn(async () => ({ values: {} })),
  vaultStateSet: vi.fn(async () => {}),
  systemInfo: vi.fn(async () => ({ platform: 'windows', arch: 'x86_64', version: '0.1.0', globalDbRebuilt: false })),
}));
vi.mock('@core/ipc/events', () => ({
  onIndexProgress: vi.fn(async () => () => {}),
  onIndexCompleted: vi.fn(async () => () => {}),
  onIndexFailed: vi.fn(async () => () => {}),
}));

/**
 * `@core/shortcut` 替身：**只做监听注册表**（记录当前还挂着几条窗口 keydown）。
 *
 * 为什么必须替身而不是用真的：本 spec 刻意不装 Pinia 的真实 feature store ——
 * 但它的 window 监听**会**继续响应后面用例派发的键盘事件（实测：单独跑绿、整套跑红）。
 * 有了注册表，"有没有悬挂监听"就成了可直接断言的事实。
 */
const shortcutRegistry = vi.hoisted(() => ({ live: 0, bindings: 0 }));
interface KeydownBinding {
  spec: string;
  handler: (event: KeyboardEvent) => void;
}
vi.mock('@core/shortcut', () => ({
  bindShortcuts: (
    target: Window | HTMLElement,
    bindings: KeydownBinding[],
    options: { preventDefault?: boolean } = {},
  ): (() => void) => {
    shortcutRegistry.live += 1;
    shortcutRegistry.bindings = bindings.length;
    const listener = (event: Event): void => {
      const keyEvent = event as KeyboardEvent;
      for (const binding of bindings) {
        if (`${binding.spec}` === 'Mod+Shift+T' && keyEvent.ctrlKey && keyEvent.shiftKey && keyEvent.key.toLowerCase() === 't') {
          if (options.preventDefault !== false) keyEvent.preventDefault();
          binding.handler(keyEvent);
          return;
        }
      }
    };
    target.addEventListener('keydown', listener);
    return () => {
      shortcutRegistry.live -= 1;
      target.removeEventListener('keydown', listener);
    };
  },
}));

import WorkspaceLayout from '@/app/layout/WorkspaceLayout.vue';

/** 挂载过的实例：用例失败时也必须卸载，否则上一轮的 watcher/监听会污染后续用例（实测）。 */
const mountedWrappers: { unmount(): void }[] = [];
afterEach(() => {
  for (const wrapper of mountedWrappers.splice(0)) wrapper.unmount();
  shortcutRegistry.live = 0;
});

async function mountWorkspace() {
  const wrapper = mount(WorkspaceLayout, { global: { plugins: [createPinia()] } });
  mountedWrappers.push(wrapper);
  await flushPromises();
  return wrapper;
}

describe('工作区：启动即恢复布局（9.5）', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    const { useVaultStore } = await import('@features/vault');
    useVaultStore().current = { ...VAULT };
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultStateGet).mockResolvedValue({ values: {} });
    fileTreeMock.expanded = new Set();
  });

  it('watch 不触发（路由守卫已 restore current）也会在挂载时读一次 vault_state', async () => {
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultStateGet).mockResolvedValue({
      values: { 'ui.sidebarWidth': 460, 'ui.sidebarCollapsed': true, 'ui.rightPanel': 'links' },
    });
    const wrapper = await mountWorkspace();
    const { useLayoutStore } = await import('@features/layout');
    const layout = useLayoutStore();
    expect(layout.sidebarWidth).toBe(460);
    expect(layout.sidebarCollapsed).toBe(true);
    expect(layout.rightPanel).toBe('links');
    expect(vi.mocked(commands.vaultStateGet)).toHaveBeenCalledWith([
      'ui.sidebarCollapsed', 'ui.sidebarWidth', 'ui.rightPanel',
    ]);
    wrapper.unmount();
  });

  it('9.7：窗口缩到 <900 自动折叠左栏（不写盘），放大后恢复用户显式设置', async () => {
    const original = Object.getOwnPropertyDescriptor(window, 'innerWidth');
    Object.defineProperty(window, 'innerWidth', { value: 800, configurable: true, writable: true });
    try {
      const commands = await import('@core/ipc/commands');
      const w = await mountWorkspace();
      const { useLayoutStore } = await import('@features/layout');
      const layout = useLayoutStore();
      expect(layout.narrow).toBe(true);
      expect(layout.effectiveCollapsed).toBe(true);
      expect((w.find('[data-testid="workspace-sidebar"]').element as HTMLElement).style.display).toBe('none');

      vi.mocked(commands.vaultStateSet).mockClear();
      window.innerWidth = 1200;
      window.dispatchEvent(new Event('resize'));
      await flushPromises();
      expect(layout.narrow).toBe(false);
      expect(layout.effectiveCollapsed).toBe(false); // 放大后回到用户显式设置（默认展开）
      // narrow 只是临时态：折叠/展开都不得写 vault_state
      expect(vi.mocked(commands.vaultStateSet)).not.toHaveBeenCalledWith({ 'ui.sidebarCollapsed': true });
      w.unmount();
    } finally {
      if (original === undefined) delete (window as unknown as { innerWidth?: number }).innerWidth;
      else Object.defineProperty(window, 'innerWidth', original);
    }
  });

  it('窗口 resize 监听只在挂载注册一次、卸载注销（9.7 的监听泄漏回归）', async () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    const removeSpy = vi.spyOn(window, 'removeEventListener');
    const wrapper = await mountWorkspace();
    const resizeAdds = addSpy.mock.calls.filter((call) => call[0] === 'resize');
    expect(resizeAdds).toHaveLength(1);
    wrapper.unmount();
    expect(removeSpy.mock.calls.filter((call) => call[0] === 'resize')).toHaveLength(1);
    addSpy.mockRestore();
    removeSpy.mockRestore();
  });
});

describe('工作区：切换知识库入口（FR-VAULT-04/05、AC-VAULT-03 的落盘）', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    dirtyFlag.value = false;
    editorMock.hasUnsaved.mockImplementation(() => false);
    fileTreeMock.expanded = new Set(['归档']);
    fileTreeMock.includeHidden = true;
    opsMock.notice = '已新建 x.md';
    // mock 是**共享**的 reactive store：上一个用例若切换过，current 会是 null，
    // 于是下一个用例连按钮都渲染不出来。每个用例都恢复成"已打开 Vault"。
    const { useVaultStore } = await import('@features/vault');
    useVaultStore().current = { ...VAULT };
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultStateGet).mockResolvedValue({ values: {} });
  });

  it('打开 Vault 时提供切换入口，并把 Vault 名显示出来', async () => {
    const w = await mountWorkspace();
    expect(w.find('[data-testid="workspace-switch-vault"]').exists()).toBe(true);
    expect(w.find('[data-testid="workspace-vault-name"]').text()).toContain('我的知识库');
    w.unmount();
  });

  it('点击切换：先刷盘再关闭，随后回到欢迎页列表', async () => {
    const w = await mountWorkspace();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(editorMock.saveAll).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="vault-list"]').exists()).toBe(true);
    w.unmount();
  });

  it('仍有未保存内容时不切换，并给出可见提示（R-07：不静默丢内容）', async () => {
    dirtyFlag.value = true;
    editorMock.hasUnsaved.mockImplementation(() => true);
    const w = await mountWorkspace();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(w.find('[data-testid="workspace-switch-error"]').text()).toContain('已取消切换');
    expect(w.find('[data-testid="vault-list"]').exists()).toBe(false);
    w.unmount();
  });

  it('切库（current→null）：清空编辑器标签/右栏面板/链接数据/文件树展开态/操作提示/渲染缓存', async () => {
    const w = await mountWorkspace();
    // 挂载本身也会 apply 一次作用域；这里只统计「点击切换」这一下的重置
    editorMock.closeAll.mockClear();
    searchHoisted.closePanel.mockClear();
    tagsHoisted.closePanel.mockClear();
    linksHoisted.reset.mockClear();
    fileTreeMock.reset.mockClear();
    opsMock.clearNotice.mockClear();
    clearRenderCacheMock.mockClear();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(editorMock.closeAll).toHaveBeenCalledTimes(1); // 串文件：不保留上一个库的标签
    expect(searchHoisted.closePanel).toHaveBeenCalled();
    expect(tagsHoisted.closePanel).toHaveBeenCalled();
    expect(linksHoisted.reset).toHaveBeenCalledTimes(1);
    expect(fileTreeMock.reset).toHaveBeenCalledTimes(1); // 串目录（「文件不存在：归档 不是目录」的根因）
    expect(fileTreeMock.expanded.size).toBe(0);
    expect(fileTreeMock.includeHidden).toBe(false);
    expect(opsMock.clearNotice).toHaveBeenCalledTimes(1); // 串提示
    expect(opsMock.notice).toBeNull();
    expect(clearRenderCacheMock).toHaveBeenCalled();
    const { useLayoutStore } = await import('@features/layout');
    expect(useLayoutStore().sidebarWidth).toBe(280); // 回默认，不带着 A 的布局进 B
    expect(useLayoutStore().rightPanel).toBe('hidden');
    w.unmount();
  });

  it('A→B 直接切换（不经过欢迎页）同样先重置、再按 B 的 vault_state 恢复', async () => {
    const commands = await import('@core/ipc/commands');
    const w = await mountWorkspace(); // A
    editorMock.closeAll.mockClear();
    linksHoisted.refresh.mockClear();
    vi.mocked(commands.vaultStateGet).mockResolvedValue({
      values: { 'ui.sidebarWidth': 500, 'ui.rightPanel': 'links' },
    });
    const { useVaultStore } = await import('@features/vault');
    useVaultStore().current = { ...VAULT, vaultId: 2, displayName: '库二' };
    await flushPromises();
    const { useLayoutStore } = await import('@features/layout');
    expect(editorMock.closeAll).toHaveBeenCalledTimes(1);
    expect(useLayoutStore().sidebarWidth).toBe(500);
    // 反链 dock 在新库里保持打开时必须按新库重查（A→B 不经过欢迎页时面板不会重新挂载）
    expect(linksHoisted.refresh).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="workspace-vault-name"]').text()).toContain('库二');
    w.unmount();
  });
});

describe('工作区：标签面板入口（FR-TAG-01 的 Mod+Shift+T）', () => {
  it('快捷键打开标签面板，面板自己的关闭事件把它收起', async () => {
    vi.clearAllMocks();
    const { useVaultStore } = await import('@features/vault');
    const { useTagsStore } = await import('@features/tags');
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultStateGet).mockResolvedValue({ values: {} });
    // 上层 describe 的"点击切换"会把 current 置空，而标签面板挂在 `v-if="vaultStore.current"` 之内：
    // 不恢复就会得到"快捷键没反应"的假红。mock 是**共享**的 reactive store，必须显式复原。
    useVaultStore().current = { ...VAULT };
    useTagsStore().panelOpen = false;
    expect(shortcutRegistry.live).toBe(0); // 前面的用例都解绑了：本用例看到的只有自己挂的监听
    const w = await mountWorkspace();
    expect(shortcutRegistry.live).toBe(1);
    expect(shortcutRegistry.bindings).toBe(4); // 布局一次绑四条：Mod+Shift+F / Mod+O / Mod+Shift+T / Mod+P
    expect(w.find('[data-testid="workspace-tag-panel"]').exists()).toBe(false);

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 't', ctrlKey: true, shiftKey: true }));
    await flushPromises();

    expect(w.find('[data-testid="workspace-tag-panel"]').exists()).toBe(true);

    await w.find('[data-testid="tag-panel-close"]').trigger('click');
    await flushPromises();
    expect(w.find('[data-testid="workspace-tag-panel"]').exists()).toBe(false);
    // 显式卸载：布局一次性绑了 4 条 window 快捷键（含命令面板的 Mod+P），留着监听会把下一个用例的键盘事件也吃掉
    w.unmount();
  });

  it('卸载时注销快捷键（ED-01：不留悬挂监听）', async () => {
    vi.clearAllMocks();
    const { useVaultStore } = await import('@features/vault');
    const { useTagsStore } = await import('@features/tags');
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultStateGet).mockResolvedValue({ values: {} });
    useVaultStore().current = { ...VAULT };
    useTagsStore().panelOpen = false;
    const w = await mountWorkspace();
    expect(shortcutRegistry.live).toBe(1);
    w.unmount();
    // ED-01 的直接证据：卸载**真的**把监听摘掉了（不是"看起来没反应"）
    expect(shortcutRegistry.live).toBe(0);
    // 卸载后同一条键盘事件不再打开面板。
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 't', ctrlKey: true, shiftKey: true }));
    await flushPromises();
    expect(useTagsStore().panelOpen).toBe(false);
  });
});
