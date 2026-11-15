import { mount, flushPromises } from '@vue/test-utils';

// 本文件在 `vi.mock` 工厂里声明了两个**替身组件**（TagPanel / FileTree 之外的那个）。
// 规则默认"一个文件一个组件"，但工厂必须自带组件定义（工厂被提升，不能引用模块作用域里的组件）——
// 这里显式豁免并留痕，而不是把替身挪出去（挪出去就绕不过 mock 注册）。
/* eslint-disable vue/one-component-per-file */

// 该 spec 挂载整个布局 → 其中的 IndexBadge / LinksPanel 会订阅索引与文件事件。
// 全局 setup 已兜底，这里仍显式 mock：把依赖写在用例里，读的人一眼能看到。
vi.mock('@core/ipc/events', () => ({
  onIndexProgress: vi.fn(async () => () => {}),
  onIndexCompleted: vi.fn(async () => () => {}),
  onIndexFailed: vi.fn(async () => () => {}),
  onFsCreated: vi.fn(async () => () => {}),
  onFsModified: vi.fn(async () => () => {}),
  onFsRemoved: vi.fn(async () => () => {}),
  onFsRenamed: vi.fn(async () => () => {}),
}));
import { defineComponent } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  vaultCurrent: vi.fn(),
  indexStatus: vi.fn(async () => ({ indexDir: 'd', indexDb: 'db', ready: true })),
  fileTree: vi.fn(async () => []),
  noteRead: vi.fn(),
  noteWrite: vi.fn(),
  preferenceGet: vi.fn(async () => ({ values: {} })),
  preferenceSet: vi.fn(),
  vaultStateGet: vi.fn(async () => ({ values: {} })),
  vaultStateSet: vi.fn(),
  fileReveal: vi.fn(),
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(async () => ({ valid: true })),
  // DEBT-20：布局挂载时会取一次系统信息（全局库是否重建过）
  systemInfo: vi.fn(async () => ({
    platform: 'windows',
    arch: 'x86_64',
    version: '0.1.0',
    globalDbRebuilt: false,
  })),
}));

vi.mock('@core/ipc/linkCommands', () => ({ linkOutgoing: vi.fn(async () => []) }));

/** E1 接线级替身：布局注入的 resolveAsset 必须带「vault root + 当前笔记 relPath」调用它。 */
const assetHoisted = vi.hoisted(() => ({
  assetUrlFor: vi.fn((root: string, note: string, src: string) => `asset://test/${root}/${note}/${src}`),
}));
vi.mock('@core/ipc/assetCommands', () => ({ assetUrlFor: assetHoisted.assetUrlFor }));

/**
 * 标签域替身：**只替 store 的对外面**（selectTag / panelOpen），面板本身替掉 ——
 * 这样"正文点标签 ⇒ 打开面板并选中"这条跨 feature 的编排能被直接断言。
 */
const tagsHoisted = vi.hoisted(() => ({ selectTag: vi.fn() }));
vi.mock('@features/tags', async () => {
  const { reactive, defineComponent } = await import('vue');
  // openPanel/closePanel 用普通函数（不是 vi.fn）：`vi.resetAllMocks()` 不会清掉实现，
  // 面板开关行为在后续用例里仍然真实（互斥断言依赖它）。
  const store = reactive({
    panelOpen: false,
    selectTag: tagsHoisted.selectTag,
    openPanel: () => { store.panelOpen = true; },
    closePanel: () => { store.panelOpen = false; },
  });
  const TagPanel = defineComponent({
    name: 'TagPanel',
    emits: ['open', 'close'],
    setup: (_p, { expose }) => {
      expose({ focusFilter: vi.fn() });
      return {};
    },
    template: '<div data-testid="tag-panel-stub" />',
  });
  return { TagPanel, useTagsStore: () => store };
});

import { vaultCurrent } from '@core/ipc/commands';
import WorkspaceLayout from '@/app/layout/WorkspaceLayout.vue';

const FileTreeStub = defineComponent({
  name: 'FileTree',
  emits: ['open', 'action'],
  template: '<div data-testid="tree-stub" />',
});

/**
 * 本 spec 会挂真实 EditorView（CodeMirror）；jsdom 里它的 rAF 测量会在用例结束后抛
 * `getClientRects is not a function`。用例显式卸载，避免遗留实例把整套测试变成 unhandled error。
 */
const openedWrappers: { unmount(): void }[] = [];
afterEach(() => {
  for (const wrapper of openedWrappers.splice(0)) wrapper.unmount();
});

function mountLayout() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const wrapper = mount(WorkspaceLayout, {
    global: { plugins: [pinia], stubs: { FileTree: FileTreeStub, EditorView: true, VaultList: true } },
  });
  openedWrappers.push(wrapper);
  return wrapper;
}

describe('WorkspaceLayout（M2 接线）', () => {
  beforeEach(() => vi.resetAllMocks());

  it('FR-GLOBAL-01 / DEBT-20：全局库重建时提示一次（含保留文件名），可关闭', async () => {
    const { systemInfo } = await import('@core/ipc/commands');
    vi.mocked(vaultCurrent).mockResolvedValue(null);
    // ① 正常启动：不出现任何提示
    const normal = mountLayout();
    await flushPromises();
    expect(normal.find('[data-testid="global-recovered"]').exists()).toBe(false);
    // ② 重建：出现提示，带上"原文件已保留"的文件名
    vi.mocked(systemInfo).mockResolvedValueOnce({
      platform: 'windows',
      arch: 'x86_64',
      version: '0.1.0',
      globalDbRebuilt: true,
      globalDbBackup: 'global.db.corrupt-1761000000',
      globalDbReason: 'file is not a database',
    });
    const rebuilt = mountLayout();
    await flushPromises();
    const banner = rebuilt.find('[data-testid="global-recovered"]');
    expect(banner.exists()).toBe(true);
    expect(banner.text()).toContain('全局库已重建');
    expect(banner.text()).toContain('global.db.corrupt-1761000000');
    // ③ 关闭后不再出现（只提示一次）
    await rebuilt.find('[data-testid="global-recovered-dismiss"]').trigger('click');
    expect(rebuilt.find('[data-testid="global-recovered"]').exists()).toBe(false);
  });
  it('未打开 Vault 时展示 Vault 列表', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue(null);
    const wrapper = mountLayout();
    await flushPromises();
    expect(wrapper.find('[data-testid="tree-stub"]').exists()).toBe(false);
    expect(wrapper.findComponent({ name: 'VaultList' }).exists() || wrapper.html().includes('vault-list')).toBe(true);
  });

  it('已打开 Vault 时展示文件树与编辑器', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true } as never);
    const wrapper = mountLayout();
    await flushPromises();
    expect(wrapper.find('[data-testid="tree-stub"]').exists()).toBe(true);
  });

  it('文件树打开笔记 → 编辑器打开（FR-FILE-10 的进入编辑态）', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true } as never);
    const { noteRead } = await import('@core/ipc/commands');
    vi.mocked(noteRead).mockResolvedValue({ relPath: 'a.md', content: 'x', mtimeMs: 1, sizeBytes: 1 });
    const wrapper = mountLayout();
    await flushPromises();
    wrapper.findComponent(FileTreeStub).vm.$emit('open', 'a.md');
    await flushPromises();
    expect(vi.mocked(noteRead)).toHaveBeenCalledWith('a.md');
  });

  it('右键动作覆盖新建笔记/文件夹/删除（含文件→取其所在目录）', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true } as never);
    const wrapper = mountLayout();
    await flushPromises();
    const tree = wrapper.findComponent(FileTreeStub);
    for (const [type, relPath, isDir] of [
      ['newNote', 'dir', true],
      ['newFolder', 'dir', true],
      ['delete', 'dir/a.md', false],
    ] as const) {
      tree.vm.$emit('action', { type, relPath, isDir });
      await flushPromises();
      expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
      const cancel = wrapper.find('[data-testid="fileops-cancel"]');
      if (cancel.exists()) await cancel.trigger('click');
    }
  });
  it('文件树右键动作 → 打开对应文件操作对话框', async () => {
    vi.mocked(vaultCurrent).mockResolvedValue({ root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true } as never);
    const wrapper = mountLayout();
    await flushPromises();
    wrapper.findComponent(FileTreeStub).vm.$emit('action', { type: 'rename', relPath: 'dir/a.md', isDir: false });
    await flushPromises();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="fileops-name"]').element as HTMLInputElement).toBeTruthy();
  });
});

describe('WorkspaceLayout：阅读态的点击路由接线（FR-EDITOR-25/27、FR-TAG-05）', () => {
  beforeEach(() => {
    // `resetAllMocks` 会把 `@features/tags` 工厂里填的 `tagsStoreMock.current` 一起抹掉
    // （它的初值就是 null）⇒ 后面再想打开面板就点了个空。用 `clearAllMocks` 只清调用记录。
    vi.clearAllMocks();
    tagsHoisted.selectTag.mockClear();
    vi.mocked(vaultCurrent).mockResolvedValue({ root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true } as never);
  });

  /** 打开一篇含扩展语法的笔记并切到阅读态（注意：**不 stub EditorView**，走真实组件）。 */
  async function mountReading(content: string) {
    const { noteRead } = await import('@core/ipc/commands');
    vi.mocked(noteRead).mockResolvedValue({ relPath: 'a.md', content, mtimeMs: 1, sizeBytes: content.length });
    const pinia = createPinia();
    setActivePinia(pinia);
    const wrapper = mount(WorkspaceLayout, {
      global: { plugins: [pinia], stubs: { FileTree: FileTreeStub, VaultList: true } },
    });
    openedWrappers.push(wrapper);
    await flushPromises();
    const { useEditorStore } = await import('@features/editor');
    await useEditorStore().openNote('a.md');
    await flushPromises();
    await wrapper.find('[data-testid="mode-read"]').trigger('click');
    await flushPromises();
    return wrapper;
  }

  it('正文点标签 ⇒ 选中该标签（FR-TAG-05；面板的开启由 store.selectTag 负责）', async () => {
    const wrapper = await mountReading('这是 #项目/alpha');
    await wrapper.find('[data-kp-tag]').trigger('click');
    await flushPromises();
    expect(tagsHoisted.selectTag).toHaveBeenCalledWith('项目/alpha');
  });

  it('Ctrl+点击已裁决链接 ⇒ 走既有 editor.reveal 通路（不新造定位机制）', async () => {
    const { linkOutgoing } = await import('@core/ipc/linkCommands');
    vi.mocked(linkOutgoing).mockResolvedValue([
      { targetRef: '笔记A', status: 'resolved', linkKind: 'wikilink', line: 1, col: 1, dstRelPath: '笔记A.md' },
    ]);
    const wrapper = await mountReading('见 [[笔记A]]');
    const { noteRead } = await import('@core/ipc/commands');
    vi.mocked(noteRead).mockClear();
    await wrapper.find('[data-kp-link]').trigger('click', { ctrlKey: true });
    await flushPromises();
    // reveal 会去读目标笔记（= 跳转真的发生了），而不是停在原地。
    // 2026-11 修复 D2/10.7：已裁决链接交给 reveal 的是**真实 relPath**（此前误传显示名 ⇒ 真机「文件不存在」）。
    expect(vi.mocked(noteRead)).toHaveBeenCalledWith('笔记A.md');
  });

  it('E1：本地图片经布局注入的 resolveAsset 渲染为 asset URL（FR-EDITOR-43）', async () => {
    const wrapper = await mountReading('![图](../assets/x.png)');
    expect(assetHoisted.assetUrlFor).toHaveBeenCalledWith('C:/v', 'a.md', '../assets/x.png');
    expect(wrapper.find('img').attributes('src')).toContain('asset://test/');
  });

  it('普通点击悬空链接 ⇒ 打开既有"新建笔记"对话框并预填建议名（FR-EDITOR-27，新增能力）', async () => {
    const { linkOutgoing } = await import('@core/ipc/linkCommands');
    vi.mocked(linkOutgoing).mockResolvedValue([
      { targetRef: '缺笔记', status: 'dangling', linkKind: 'wikilink', line: 1, col: 1, dstRelPath: null },
    ]);
    const wrapper = await mountReading('见 [[缺笔记]]');
    await wrapper.find('[data-kp-link]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
    expect((wrapper.find('[data-testid="fileops-name"]').element as HTMLInputElement).value).toBe('缺笔记.md');
  });
});

/**
 * 9.6：单一右栏 dock 与 Ribbon 开关。
 *
 * 真机反馈：搜索与标签会并排打开、再叠上反链，三个栏一起压在编辑区上；
 * Ribbon 的搜索/标签只能打开不能关闭。这三条在这里用真实 store + 真实面板做回归。
 */
describe('WorkspaceLayout：单一右栏 dock 与 Ribbon 开关（9.6）', () => {
  const VAULT = { root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true } as never;
  beforeEach(async () => {
    vi.clearAllMocks();
    // vault 状态通路的默认实现（resetAllMocks 会清掉工厂里的实现）
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultCurrent).mockResolvedValue(VAULT);
    vi.mocked(commands.vaultStateGet).mockResolvedValue({ values: {} } as never);
    const { useTagsStore } = await import('@features/tags');
    useTagsStore().panelOpen = false;
    const { useSearchStore } = await import('@features/search');
    useSearchStore().panelOpen = false;
    const { useLayoutStore } = await import('@features/layout');
    useLayoutStore().reset();
  });

  async function mountWithVault() {
    const wrapper = mountLayout();
    await flushPromises();
    return wrapper;
  }

  it('右侧面板默认关闭：不渲染 dock，也不渲染反链', async () => {
    const wrapper = await mountWithVault();
    expect(wrapper.find('[data-testid="workspace-right-dock"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="search-panel"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it('Ribbon 搜索是开关：点一次打开、再点一次关闭', async () => {
    const wrapper = await mountWithVault();
    await wrapper.get('[data-testid="ribbon-search"]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="workspace-right-dock"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="search-panel"]').exists()).toBe(true);
    expect(wrapper.get('[data-testid="ribbon-search"]').attributes('aria-pressed')).toBe('true');

    await wrapper.get('[data-testid="ribbon-search"]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="workspace-right-dock"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="ribbon-search"]').attributes('aria-pressed')).toBe('false');
    wrapper.unmount();
  });

  it('搜索与标签互斥：打开标签会关闭搜索，右栏始终只有一个面板', async () => {
    const wrapper = await mountWithVault();
    await wrapper.get('[data-testid="ribbon-search"]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="search-panel"]').exists()).toBe(true);

    await wrapper.get('[data-testid="ribbon-tags"]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="workspace-tag-panel"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="search-panel"]').exists()).toBe(false);
    expect(wrapper.findAll('[data-testid="workspace-right-dock"]')).toHaveLength(1);

    // 再点标签 → 关闭（同样是开关）
    await wrapper.get('[data-testid="ribbon-tags"]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="workspace-right-dock"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it('Ribbon ▥：显示反链并隐藏搜索/标签；反链可见时再点全部隐藏（持久化 hidden）', async () => {
    const commands = await import('@core/ipc/commands');
    const { STATE_KEY_RIGHT_PANEL } = await import('@features/layout');
    const wrapper = await mountWithVault();

    // 先打开搜索 → 再点 ▥：关闭搜索并显示反链
    await wrapper.get('[data-testid="ribbon-search"]').trigger('click');
    await flushPromises();
    await wrapper.get('[data-testid="ribbon-right"]').trigger('click');
    await flushPromises();
    expect(wrapper.get('[data-testid="workspace-right-dock"]').text()).toContain('反链');
    expect(wrapper.find('[data-testid="search-panel"]').exists()).toBe(false);

    // 反链可见时再点 ▥ → 全部隐藏，并把 hidden 落盘（重启/切库后不会自己又打开）
    await wrapper.get('[data-testid="ribbon-right"]').trigger('click');
    await flushPromises();
    expect(wrapper.find('[data-testid="workspace-right-dock"]').exists()).toBe(false);
    expect(vi.mocked(commands.vaultStateSet)).toHaveBeenCalledWith({ [STATE_KEY_RIGHT_PANEL]: 'hidden' });
    wrapper.unmount();
  });

  it('状态异常（搜索与标签同为 true）时也只渲染一个面板，不重叠', async () => {
    const wrapper = await mountWithVault();
    const { useSearchStore } = await import('@features/search');
    const { useTagsStore } = await import('@features/tags');
    useSearchStore().panelOpen = true;
    useTagsStore().panelOpen = true;
    await flushPromises();
    expect(wrapper.findAll('[data-testid="workspace-right-dock"]')).toHaveLength(1);
    expect(wrapper.find('[data-testid="search-panel"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="workspace-tag-panel"]').exists()).toBe(false);
    wrapper.unmount();
  });
});

/**
 * 7.5：命令面板「保存当前笔记」失败时必须保持打开并显示错误。
 *
 * 根因（真机）：`editor.save` 把错误收进 buffer.error 后正常 resolve，旧执行体
 * `if (relPath) return editor.save(relPath)` 于是被 palette.execute 当成成功 → 关闭面板、零提示。
 */
describe('WorkspaceLayout：7.5 保存失败时命令面板保持打开并显示错误', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    const commands = await import('@core/ipc/commands');
    vi.mocked(commands.vaultCurrent).mockResolvedValue({
      root: 'C:/v', displayName: '库', vaultId: 1, caseInsensitiveFs: true,
    } as never);
    vi.mocked(commands.vaultStateGet).mockResolvedValue({ values: {} } as never);
    vi.mocked(commands.systemInfo).mockResolvedValue({
      platform: 'windows', arch: 'x86_64', version: '0.1.0', globalDbRebuilt: false,
    } as never);
    const { useSearchStore } = await import('@features/search');
    useSearchStore().panelOpen = false;
    const { useTagsStore } = await import('@features/tags');
    useTagsStore().panelOpen = false;
  });

  it('外部删除后保存：面板不关闭，红字显示错误，buffer.error 有原因', async () => {
    const commands = await import('@core/ipc/commands');
    const { noteRead, noteWrite } = commands;
    vi.mocked(noteRead).mockResolvedValue({ relPath: 'a.md', content: '旧内容', mtimeMs: 1, sizeBytes: 6 } as never);
    vi.mocked(noteWrite).mockRejectedValue({ code: 'E_NOTE_NOT_FOUND', message: '文件不存在：a.md' });

    const wrapper = mountLayout();
    await flushPromises();
    const { useEditorStore } = await import('@features/editor');
    const editor = useEditorStore();
    await editor.openNote('a.md');
    editor.updateContent('a.md', '改过的内容');
    await flushPromises();

    const { usePaletteStore } = await import('@features/palette');
    const palette = usePaletteStore();
    palette.open({ hasActiveNote: true, hasVault: true, mode: 'edit', includeHidden: false });
    palette.setQuery('保存当前笔记');
    const index = palette.results.findIndex((command) => command.id === 'palette.save-note');
    expect(index).toBeGreaterThanOrEqual(0);

    const ok = await palette.execute(index);
    await flushPromises();

    expect(ok).toBe(false);
    expect(palette.isOpen).toBe(true); // 面板保持打开
    expect(palette.error).toContain('文件不存在');
    expect(wrapper.find('[data-testid="palette-error"]').text()).toContain('文件不存在'); // 底部红字
    expect(editor.buffers.find((buffer) => buffer.relPath === 'a.md')?.error).toBeTruthy();
    wrapper.unmount();
  });
});
