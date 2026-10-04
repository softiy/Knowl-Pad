import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
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

import {
  fileReveal,
  fileTree,
  preferenceGet,
  preferenceSet,
  vaultStateGet,
  vaultStateSet,
  type FileNode,
} from '@core/ipc/commands';
import { FileTree } from '@features/files';

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

function mountTree() {
  return mount(FileTree, { global: { plugins: [createPinia()] } });
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
    const icons = renderedRows(wrapper).map((r) => r.find('.file-tree__icon').text());
    expect(icons).toEqual(['📎', '📄']);
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
