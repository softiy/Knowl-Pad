import { mount, flushPromises } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

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

// 工作区布局现在会挂 LinksPanel（链接域），它 onMounted 就查 IPC —— 这里整体替身掉，
// 让本 spec 只关心"切换知识库"这一条链路（依赖最小化）。
vi.mock('@features/links', () => ({
  LinksPanel: { name: 'LinksPanel', template: '<div data-testid="links-panel" />' },
}));

const saveAll = vi.fn(async () => {});
const dirtyFlag = { value: false };
const hasUnsaved = vi.fn(() => false);
const buffers = [{ relPath: 'a.md' }];
vi.mock('@features/editor', async () => {
  return {
    EditorView: { template: '<div data-testid="editor" />' },
    useEditorStore: () => ({
      buffers,
      saveAll,
      hasUnsaved,
      openNote: vi.fn(async () => {}),
      isDirty: vi.fn(() => dirtyFlag.value),
    }),
  };
});

// 搜索域同样整体替身：本 spec 刻意不装 Pinia（每个 feature 都用替身），
// 而 SearchPanel/QuickOpen 的真身要读 Pinia store —— 这里只保留"布局用到的那几个面"。
vi.mock('@features/search', async () => {
  const { reactive } = await import('vue');
  const store = reactive({
    panelOpen: false,
    quickOpenVisible: false,
    openPanel: vi.fn(() => {
      store.panelOpen = true;
    }),
    closePanel: vi.fn(() => {
      store.panelOpen = false;
    }),
    openQuickOpen: vi.fn(() => {
      store.quickOpenVisible = true;
    }),
    closeQuickOpen: vi.fn(() => {
      store.quickOpenVisible = false;
    }),
  });
  return {
    SearchPanel: { name: 'SearchPanel', template: '<div data-testid="search-panel" />' },
    QuickOpen: { name: 'QuickOpen', template: '<div data-testid="quick-open" />' },
    useSearchStore: () => store,
  };
});

// 标签域（M5 批次 2 的 PR-C）同样是整体替身：本 spec 刻意不装 Pinia，
// 而 TagPanel 的真身要读 Pinia store —— 这里只保留"布局用到的那几个面"
// （panelOpen / openPanel / closePanel / 面板的 close 事件）。
vi.mock('@features/tags', async () => {
  const { reactive } = await import('vue');
  const store = reactive({
    panelOpen: false,
    openPanel: vi.fn(() => {
      store.panelOpen = true;
    }),
    closePanel: vi.fn(() => {
      store.panelOpen = false;
    }),
  });
  const { defineComponent } = await import('vue');
  // 替身必须复刻真身的**对外 API**（`defineExpose` 的 focusFilter）：
  // 少了它，布局的"打开面板后聚焦过滤框"会在运行期炸出 unhandled rejection
  // —— 那是替身不忠实，而不是被测代码有问题。
  // 可见性判据用**布局给的** `workspace-tag-panel`：替身根元素上的同名属性会被透传属性覆盖，
  // 拿替身自己的 testid 去找是找不到的（实测踩过）。
  const TagPanel = defineComponent({
    name: 'TagPanel',
    emits: ['open', 'close'],
    setup: (_props, { expose, emit }) => {
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

vi.mock('@features/file-tree', () => ({
  FileTree: { template: '<div />' },
  FileOpsDialog: { template: '<div />' },
  useFileOpsStore: () => ({ notice: null, openNewNote: vi.fn(), openNewFolder: vi.fn(), openRename: vi.fn(), openDelete: vi.fn() }),
}));
vi.mock('@features/vault/components/VaultList.vue', () => ({ default: { template: '<div data-testid="vault-list" />' } }));
vi.mock('@core/ipc/commands', () => ({ indexStatus: vi.fn(async () => ({ ready: true })), fileTree: vi.fn(async () => []), preferenceGet: vi.fn(async () => ({ values: {} })), preferenceSet: vi.fn() }));
vi.mock('@core/ipc/events', () => ({ onIndexProgress: vi.fn(async () => () => {}), onIndexCompleted: vi.fn(async () => () => {}), onIndexFailed: vi.fn(async () => () => {}) }));


/**
 * `@core/shortcut` 替身：**只做监听注册表**（记录当前还挂着几条窗口 keydown）。
 *
 * 为什么必须替身而不是用真的：本 spec 刻意不装 Pinia、各 feature 一律替身，于是"挂载了却没卸载"
 * 的实例不会污染任何真实 store —— 但它的 window 监听**会**继续响应后面用例派发的键盘事件
 * （实测：单独跑绿、整套跑红）。有了注册表，"有没有悬挂监听"就成了可直接断言的事实。
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

describe('工作区：切换知识库入口（FR-VAULT-04/05、AC-VAULT-03 的落盘）', () => {
  beforeEach(async () => {
    saveAll.mockClear();
    hasUnsaved.mockReset();
    hasUnsaved.mockReturnValue(false);
    // mock 是**共享**的 reactive store：上一个用例若切换过，current 会是 null，
    // 于是下一个用例连按钮都渲染不出来。每个用例都恢复成"已打开 Vault"。
    const { useVaultStore } = await import('@features/vault');
    useVaultStore().current = { ...VAULT };
  });

  it('打开 Vault 时提供切换入口，并把 Vault 名显示出来', async () => {
    const w = mount(WorkspaceLayout, { global: { plugins: [createPinia()] } });
    await flushPromises();
    expect(w.find('[data-testid="workspace-switch-vault"]').exists()).toBe(true);
    expect(w.find('[data-testid="workspace-vault-name"]').text()).toContain('我的知识库');
    w.unmount();
  });

  it('点击切换：先刷盘再关闭，随后回到欢迎页列表', async () => {
    const w = mount(WorkspaceLayout, { global: { plugins: [createPinia()] } });
    await flushPromises();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(saveAll).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="vault-list"]').exists()).toBe(true);
    w.unmount();
  });

  it('仍有未保存内容时不切换，并给出可见提示（R-07：不静默丢内容）', async () => {
    hasUnsaved.mockReturnValue(true);
    dirtyFlag.value = true;
    const w = mount(WorkspaceLayout, { global: { plugins: [createPinia()] } });
    await flushPromises();
    await w.find('[data-testid="workspace-switch-vault"]').trigger('click');
    await flushPromises();
    expect(w.find('[data-testid="workspace-switch-error"]').text()).toContain('已取消切换');
    expect(w.find('[data-testid="vault-list"]').exists()).toBe(false);
    dirtyFlag.value = false;
    w.unmount();
  });
});

describe('工作区：标签面板入口（FR-TAG-01 的 Mod+Shift+T）', () => {
  it('快捷键打开标签面板，面板自己的关闭事件把它收起', async () => {
    const { useVaultStore } = await import('@features/vault');
    const { useTagsStore } = await import('@features/tags');
    // 上层 describe 的"点击切换"会把 current 置空，而标签面板挂在 `v-if="vaultStore.current"` 之内：
    // 不恢复就会得到"快捷键没反应"的假红（实测踩过）。mock 是**共享**的 reactive store，必须显式复原。
    useVaultStore().current = { ...VAULT };
    useTagsStore().panelOpen = false;
    expect(shortcutRegistry.live).toBe(0); // 前面的用例都解绑了：本用例看到的只有自己挂的监听
    const w = mount(WorkspaceLayout, { global: { plugins: [createPinia()] } });
    await flushPromises();
    expect(shortcutRegistry.live).toBe(1);
    expect(shortcutRegistry.bindings).toBe(4); // 布局一次绑四条：Mod+Shift+F / Mod+O / Mod+Shift+T / Mod+P（命令面板）
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
    const { useVaultStore } = await import('@features/vault');
    const { useTagsStore } = await import('@features/tags');
    useVaultStore().current = { ...VAULT };
    useTagsStore().panelOpen = false;
    const w = mount(WorkspaceLayout, { global: { plugins: [createPinia()] } });
    await flushPromises();
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
