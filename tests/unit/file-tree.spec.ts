import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * 虚拟滚动的**受控替身**：jsdom 没有布局（offsetHeight 恒为 0），真实 virtualizer 无法工作，
 * 因此这里用一个确定性实现替换它——只验证**我们这一侧**的集成不变量（行数有界、窗口随滚动移动、
 * 反复滚动不增长），算法本身由库负责。
 */
vi.mock('@tanstack/vue-virtual', async () => {
  const { computed, reactive, unref } = await import('vue');
  const state = reactive({ scrollTop: 0, viewport: 280 });
  (globalThis as unknown as Record<string, unknown>).__treeVirtualState = state;
  return {
    useVirtualizer: (opts: unknown) => {
      const items = computed(() => {
        const o = unref(opts) as { count: number; estimateSize: () => number; overscan?: number };
        const row = o.estimateSize();
        const overscan = o.overscan ?? 0;
        const visible = Math.ceil(state.viewport / row);
        const start = Math.floor(state.scrollTop / row);
        const from = Math.max(0, start - overscan);
        const to = Math.min(o.count, start + visible + overscan);
        return Array.from({ length: Math.max(0, to - from) }, (_, i) => ({
          index: from + i,
          key: from + i,
          start: (from + i) * row,
          size: row,
        }));
      });
      return computed(() => ({
        getVirtualItems: () => items.value,
        getTotalSize: () => (unref(opts) as { count: number }).count * 28,
      }));
    },
  };
});

vi.mock('@core/ipc/commands', () => ({
  fileTree: vi.fn(),
  fileReveal: vi.fn(),
  preferenceGet: vi.fn(),
  preferenceSet: vi.fn(),
  vaultStateGet: vi.fn(),
  vaultStateSet: vi.fn(),
}));

/**
 * kp://fs/* 的受控替身：FileTree 挂载时订阅、卸载时注销。
 * 用例直接调用捕获到的 handler 来模拟"后端文件系统事件"，不依赖 Tauri 运行时。
 */
const fsHandlers = vi.hoisted(() => ({
  modified: null as ((payload: { relPath: string; mtimeMs: number }) => void) | null,
  created: null as ((payload: { relPath: string; kind: string }) => void) | null,
  removed: null as ((payload: { relPath: string }) => void) | null,
  renamed: null as ((payload: { from: string; to: string }) => void) | null,
  unlisten: vi.fn(),
}));
vi.mock('@core/ipc/events', () => ({
  onFsModified: (handler: (payload: { relPath: string; mtimeMs: number }) => void) => {
    fsHandlers.modified = handler;
    return Promise.resolve(fsHandlers.unlisten);
  },
  onFsCreated: (handler: (payload: { relPath: string; kind: string }) => void) => {
    fsHandlers.created = handler;
    return Promise.resolve(fsHandlers.unlisten);
  },
  onFsRemoved: (handler: (payload: { relPath: string }) => void) => {
    fsHandlers.removed = handler;
    return Promise.resolve(fsHandlers.unlisten);
  },
  onFsRenamed: (handler: (payload: { from: string; to: string }) => void) => {
    fsHandlers.renamed = handler;
    return Promise.resolve(fsHandlers.unlisten);
  },
}));

import {
  fileReveal,
  fileTree,
  preferenceGet,
  preferenceSet,
  vaultStateGet,
  vaultStateSet,
  type FileNode,
} from '@core/ipc/commands';
import { FileTree, useFileOpsStore } from '@features/file-tree';

const mTree = vi.mocked(fileTree);
const vstate = (globalThis as unknown as { __treeVirtualState: { scrollTop: number; viewport: number } })
  .__treeVirtualState;

const MAX_ROWS = Math.ceil(vstate.viewport / 28) + 2 * 10; // 可视 + 上下各 10 行缓冲

function note(relPath: string): FileNode {
  return { relPath, name: relPath, isDir: false, kind: 'note' };
}
function dir(relPath: string): FileNode {
  return { relPath, name: relPath, isDir: true, kind: 'other', hasChildren: true };
}

/** 最近一次 mount 的 fileOps store（复制提示等断言需要读它） */
let ops: ReturnType<typeof useFileOpsStore>;

function mountTree() {
  const pinia = createPinia();
  setActivePinia(pinia);
  ops = useFileOpsStore();
  return mount(FileTree, { global: { plugins: [pinia] } });
}

const renderedRows = (wrapper: ReturnType<typeof mountTree>) =>
  wrapper.findAll('[data-testid="tree-row"]');

describe('FileTree 组件', () => {
  beforeEach(() => {
    // resetAllMocks：避免上一个用例的 vaultStateGet（展开 'dir'）泄漏到下一个用例
    vi.resetAllMocks();
    vstate.scrollTop = 0;
    vi.mocked(preferenceGet).mockResolvedValue({ values: {} });
    vi.mocked(vaultStateGet).mockResolvedValue({ values: {} });
    vi.mocked(vaultStateSet).mockResolvedValue(undefined);
    vi.mocked(preferenceSet).mockResolvedValue(undefined);
    mTree.mockResolvedValue([dir('dir'), note('root.md')]);
  });

  it('AC-FILE-06：10 万节点下只渲染可视窗口（行数有界）', async () => {
    mTree.mockResolvedValue(Array.from({ length: 100_000 }, (_, i) => note(`f${i}.md`)));
    const wrapper = mountTree();
    await flushPromises();
    const count = renderedRows(wrapper).length;
    expect(count).toBeGreaterThan(0);
    expect(count).toBeLessThanOrEqual(MAX_ROWS);
  });

  it('AC-FILE-06：滚动时窗口跟随移动，且行数始终有界、反复滚动不增长', async () => {
    mTree.mockResolvedValue(Array.from({ length: 100_000 }, (_, i) => note(`f${i}.md`)));
    const wrapper = mountTree();
    await flushPromises();

    for (const offset of [1000, 5000, 20_000, 99_000]) {
      vstate.scrollTop = 28 * offset;
      await nextTick();
      expect(renderedRows(wrapper).length).toBeLessThanOrEqual(MAX_ROWS);
    }
    vstate.scrollTop = 28 * 5000;
    await nextTick();
    expect(wrapper.find('.file-tree__name').text()).toBe('f4990.md');

    for (let i = 0; i < 10; i += 1) {
      vstate.scrollTop = 28 * i * 1000;
      await nextTick();
      expect(renderedRows(wrapper).length).toBeLessThanOrEqual(MAX_ROWS);
    }
  });

  it('AC-FILE-08：从 vault_state 恢复展开层级后，子行被渲染', async () => {
    vi.mocked(vaultStateGet).mockResolvedValue({ values: { 'tree.expanded': ['dir'] } });
    mTree.mockImplementation(async (parent?: string) =>
      parent === 'dir' ? [note('dir/a.md')] : [dir('dir'), note('root.md')],
    );
    const wrapper = mountTree();
    await flushPromises();
    const names = renderedRows(wrapper).map((r) => r.find('.file-tree__name').text());
    expect(names).toContain('dir');
    expect(names).toContain('dir/a.md');
  });

  it('点击目录行展开、点击文件行向外抛 open', async () => {
    mTree.mockImplementation(async (parent?: string) =>
      parent === 'dir' ? [note('dir/a.md')] : [dir('dir'), note('root.md')],
    );
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[0].trigger('click');
    await flushPromises();
    expect(renderedRows(wrapper).map((r) => r.find('.file-tree__name').text())).toContain('dir/a.md');
    await renderedRows(wrapper)[0].trigger('click'); // 折叠
    await flushPromises();
    const fileRow = renderedRows(wrapper).find((r) => r.find('.file-tree__name').text() === 'root.md');
    await fileRow?.trigger('click');
    expect(wrapper.emitted('open')?.[0]).toEqual(['root.md']);
  });

  it('右键菜单：在文件管理器中显示与复制相对路径（FR-FILE-07）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const wrapper = mountTree();
    await flushPromises();

    await renderedRows(wrapper)[0].trigger('contextmenu');
    const reveal = wrapper.findAll('.kp-menu button').find((b) => b.text() === '在文件管理器中显示');
    await reveal?.trigger('click');
    await flushPromises();
    expect(fileReveal).toHaveBeenCalledWith('dir');

    await renderedRows(wrapper)[0].trigger('contextmenu');
    const copy = wrapper.findAll('.kp-menu button').find((b) => b.text() === '复制相对路径');
    await copy?.trigger('click');
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith('dir');
  });

  it('图标按类型区分：附件与其它文件（FR-FILE-03）', async () => {
    mTree.mockResolvedValue([
      { relPath: 'img.png', name: 'img.png', isDir: false, kind: 'attachment' },
      { relPath: 'data.txt', name: 'data.txt', isDir: false, kind: 'other' },
    ]);
    const wrapper = mountTree();
    await flushPromises();
    // 按**名字**断言图标（默认排序是名称，行序不再等于服务端返回序；FR-FILE-02）
    const iconOf = (name: string) =>
      renderedRows(wrapper)
        .find((r) => r.find('.file-tree__name').text() === name)
        ?.find('.file-tree__icon')
        .text();
    expect(iconOf('img.png')).toBe('📎');
    expect(iconOf('data.txt')).toBe('📄');
  });

  it('FR-FILE-02：排序下拉存在，切到「修改时间」按 mtime 重排（文件夹仍优先）', async () => {
    mTree.mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note', mtimeMs: 100 },
      { relPath: 'b.md', name: 'b.md', isDir: false, kind: 'note', mtimeMs: 900 },
      { relPath: 'zdir', name: 'zdir', isDir: true, kind: 'other', hasChildren: true },
    ]);
    const wrapper = mountTree();
    await flushPromises();
    const names = () => renderedRows(wrapper).map((r) => r.find('.file-tree__name').text());
    expect(names()).toEqual(['zdir', 'a.md', 'b.md']);
    await wrapper.find('[data-testid="tree-sort"]').setValue('mtime');
    await flushPromises();
    expect(names()).toEqual(['zdir', 'b.md', 'a.md']);
  });

  it('FR-FILE-07：复制 Wikilink（笔记去 .md；目录不提供该入口）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[0].trigger('contextmenu');
    expect(wrapper.find('[data-testid="menu-copy-wikilink"]').exists()).toBe(false);
    await renderedRows(wrapper)[1].trigger('contextmenu');
    await wrapper.find('[data-testid="menu-copy-wikilink"]').trigger('click');
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith('[[root]]');
    // M5 冒烟 5.5：复制成功必须给出**含复制内容**的可见提示（action 之前完全静默）
    expect(ops.notice).toBe('已复制 Wikilink：[[root]]');
    ops.clearNotice();
  });

  it('DEBT-19：右键「移动到…」向外抛 move 动作（file_move 的 UI 入口）', async () => {
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[1].trigger('contextmenu');
    await wrapper.find('[data-testid="menu-move"]').trigger('click');
    expect(wrapper.emitted('action')?.[0]).toEqual([{ type: 'move', relPath: 'root.md', isDir: false }]);
  });

  it('右键菜单：目录显示新建项，选择后向外抛 action', async () => {
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[0].trigger('contextmenu');
    const buttons = wrapper.findAll('.kp-menu button').map((b) => b.text());
    expect(buttons).toContain('新建笔记');
    expect(buttons).toContain('复制相对路径');
    await wrapper.findAll('.kp-menu button')[0].trigger('click');
    expect(wrapper.emitted('action')?.[0]).toEqual([{ type: 'newNote', relPath: 'dir', isDir: true }]);
  });
});

describe('FileTree 左栏布局与复制反馈（M5 冒烟 5.3/5.5 的回归）', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vstate.scrollTop = 0;
    vi.mocked(preferenceGet).mockResolvedValue({ values: {} });
    vi.mocked(vaultStateGet).mockResolvedValue({ values: {} });
    vi.mocked(vaultStateSet).mockResolvedValue(undefined);
    vi.mocked(preferenceSet).mockResolvedValue(undefined);
    mTree.mockResolvedValue([dir('dir'), note('root.md')]);
  });

  it('头部单行：隐藏文件开关 + 排序下拉 + 方向按钮在同一工具条内', async () => {
    const wrapper = mountTree();
    await flushPromises();
    const toolbar = wrapper.find('.file-tree__toolbar');
    expect(toolbar.exists()).toBe(true);
    expect(toolbar.find('input[type="checkbox"]').exists()).toBe(true);
    expect(toolbar.find('[data-testid="tree-sort"]').exists()).toBe(true);
    expect(toolbar.find('[data-testid="tree-sort-dir"]').exists()).toBe(true);
    // 三个控件都在**同一个**工具条里（不再被挤到第二行）
    expect(wrapper.findAll('.file-tree__toolbar')).toHaveLength(1);
  });

  it('方向按钮：名称模式默认 ↑，点击后变 ↓（纯前端翻转）', async () => {
    const wrapper = mountTree();
    await flushPromises();
    const button = wrapper.find('[data-testid="tree-sort-dir"]');
    expect(button.text()).toBe('↑');
    await button.trigger('click');
    expect(button.text()).toBe('↓');
  });

  it('每一行都带 title = 完整名称（窄侧栏下悬浮可见被截断的名字）', async () => {
    mTree.mockResolvedValue([note('一个非常非常长的笔记名字用于测试截断.md'), note('root.md')]);
    const wrapper = mountTree();
    await flushPromises();
    for (const row of renderedRows(wrapper)) {
      const title = row.attributes('title');
      expect(title).toBeTruthy();
      expect(row.find('.file-tree__name').text()).toBe(title);
    }
  });

  it('缩进更紧凑：每层 10px（而不是 12px）', async () => {
    mTree.mockImplementation(async (parent?: string) =>
      parent === 'dir' ? [note('dir/a.md')] : [dir('dir'), note('root.md')],
    );
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[0].trigger('click'); // 展开 dir
    await flushPromises();
    const child = renderedRows(wrapper).find((r) => r.find('.file-tree__name').text() === 'dir/a.md');
    expect(child?.find('.file-tree__indent').attributes('style')).toContain('width: 10px');
  });

  it('复制相对路径同样给出提示（不静默）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[1].trigger('contextmenu');
    const copy = wrapper.findAll('.kp-menu button').find((b) => b.text() === '复制相对路径');
    await copy?.trigger('click');
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith('root.md');
    expect(ops.notice).toContain('root.md');
    ops.clearNotice();
  });

  it('剪贴板不可用时如实报错（R-15），不假装复制成功', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
    const wrapper = mountTree();
    await flushPromises();
    await renderedRows(wrapper)[1].trigger('contextmenu');
    await wrapper.find('[data-testid="menu-copy-wikilink"]').trigger('click');
    await flushPromises();
    expect(ops.notice).toContain('失败');
    expect(wrapper.find('.file-tree__error').exists()).toBe(true);
    ops.clearNotice();
  });
});

describe('FileTree 订阅 kp://fs/* 事件（M5 冒烟 5.3：运行期外部改动的实时同步）', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vstate.scrollTop = 0;
    vi.mocked(preferenceGet).mockResolvedValue({ values: {} });
    vi.mocked(vaultStateGet).mockResolvedValue({ values: {} });
    vi.mocked(vaultStateSet).mockResolvedValue(undefined);
    vi.mocked(preferenceSet).mockResolvedValue(undefined);
    mTree.mockResolvedValue([dir('dir'), note('root.md')]);
    fsHandlers.modified = null;
    fsHandlers.created = null;
    fsHandlers.removed = null;
    fsHandlers.renamed = null;
  });

  it('modified：用事件里的磁盘 mtime 更新节点并重排（切到修改时间后新顺序立刻生效）', async () => {
    mTree.mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', isDir: false, kind: 'note', mtimeMs: 100 },
      { relPath: 'b.md', name: 'b.md', isDir: false, kind: 'note', mtimeMs: 900 },
    ]);
    const wrapper = mountTree();
    await flushPromises();
    await wrapper.find('[data-testid="tree-sort"]').setValue('mtime');
    expect(renderedRows(wrapper).map((r) => r.find('.file-tree__name').text())).toEqual(['b.md', 'a.md']);
    fsHandlers.modified?.({ relPath: 'a.md', mtimeMs: 5_000 });
    await nextTick();
    expect(renderedRows(wrapper).map((r) => r.find('.file-tree__name').text())).toEqual(['a.md', 'b.md']);
    wrapper.unmount();
  });

  it('created：定向重拉父层，新建文件立刻出现在树里', async () => {
    const wrapper = mountTree();
    await flushPromises();
    mTree.mockResolvedValue([dir('dir'), note('新建.md'), note('root.md')]);
    fsHandlers.created?.({ relPath: '新建.md', kind: 'note' });
    await flushPromises();
    expect(renderedRows(wrapper).map((r) => r.find('.file-tree__name').text())).toContain('新建.md');
    wrapper.unmount();
  });

  it('removed：外部删除后节点从树里消失', async () => {
    const wrapper = mountTree();
    await flushPromises();
    fsHandlers.removed?.({ relPath: 'root.md' });
    await nextTick();
    expect(renderedRows(wrapper).map((r) => r.find('.file-tree__name').text())).not.toContain('root.md');
    wrapper.unmount();
  });

  it('renamed：外部重命名后旧名消失、新名出现', async () => {
    const wrapper = mountTree();
    await flushPromises();
    fsHandlers.renamed?.({ from: 'root.md', to: 'zzz.md' });
    await nextTick();
    const names = renderedRows(wrapper).map((r) => r.find('.file-tree__name').text());
    expect(names).not.toContain('root.md');
    expect(names).toContain('zzz.md');
    wrapper.unmount();
  });

  it('卸载时注销全部订阅（ED-01：组件不留下全局监听）', async () => {
    const wrapper = mountTree();
    await flushPromises();
    wrapper.unmount();
    expect(fsHandlers.unlisten).toHaveBeenCalledTimes(4);
  });
});
