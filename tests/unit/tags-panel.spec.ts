import { flushPromises, mount } from '@vue/test-utils';
import type { DOMWrapper } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

/**
 * 标签面板（FR-TAG-01~04 / 13）：树形与计数、点击看笔记、排序切换、面板内过滤、
 * 「未分类」条目、以及 FR-TAG-03 的"含子级"开关**如实标注 no-op**（勘误 D-40②）。
 *
 * 依赖整体替身（照 `tests/unit/links-panel.spec.ts` 的风格）：本 spec 只关心面板行为，
 * 不碰真 IPC、不碰真事件总线。
 */
const hoisted = vi.hoisted(() => ({
  tree: vi.fn(),
  notes: vi.fn(),
  renamePreview: vi.fn(),
  renameApply: vi.fn(),
}));

vi.mock('@core/ipc/tagCommands', () => ({
  tagTree: hoisted.tree,
  tagNotes: hoisted.notes,
  tagRenamePreview: hoisted.renamePreview,
  tagRenameApply: hoisted.renameApply,
}));
vi.mock('@core/ipc/events', () => ({
  onIndexCompleted: vi.fn(async () => () => {}),
  onFsCreated: vi.fn(async () => () => {}),
  onFsModified: vi.fn(async () => () => {}),
  onFsRemoved: vi.fn(async () => () => {}),
  onFsRenamed: vi.fn(async () => () => {}),
}));

import TagPanel from '@features/tags/components/TagPanel.vue';
import TagRenameDialog from '@features/tags/components/TagRenameDialog.vue';
import { useTagsStore } from '@features/tags/stores/tags';

/** AC-TAG-01 的树 + 「未分类」（计数是**祖先登记后的行数**：项目 = 3 已含两个子）。 */
function treeFixture() {
  return [
    {
      norm: '项目', display: '项目', depth: 1, isLeaf: false, refCount: 3, unclassified: false,
      children: [
        { norm: '项目/alpha', display: 'alpha', depth: 2, isLeaf: true, refCount: 2, unclassified: false, children: [] },
        { norm: '项目/beta', display: 'beta', depth: 2, isLeaf: true, refCount: 1, unclassified: false, children: [] },
      ],
    },
    { norm: '随笔', display: '随笔', depth: 1, isLeaf: true, refCount: 9, unclassified: false, children: [] },
    { norm: '', display: '未分类', depth: 0, isLeaf: true, refCount: 4, unclassified: true, children: [] },
  ];
}

/** 按 `data-norm` 找树行（找不到直接抛：让用例失败在"行都没渲染"这一步，而不是模糊的空断言）。 */
function findRow(w: ReturnType<typeof mount>, norm: string): DOMWrapper<Element> {
  const row = w.findAll('[data-testid="tag-tree-row"]').find((n) => n.attributes('data-norm') === norm);
  if (!row) throw new Error('找不到行：' + norm);
  return row;
}

describe('标签面板：树形与计数（FR-TAG-01/02、AC-TAG-01）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.tree.mockReset().mockResolvedValue(treeFixture());
    hoisted.notes.mockReset().mockResolvedValue([]);
  });

  it('按层级缩进渲染（a → b → c 三级嵌套），数据里带 depth', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    // 缺省按引用计数排序：随笔 9、项目 3、项目/alpha 2、项目/beta 1、未分类（恒最后）
    const rows = w.findAll('[data-testid="tag-tree-row"]');
    expect(rows.map((r) => r.attributes('data-depth'))).toEqual(['1', '1', '2', '2', '1']);
    expect(findRow(w, '项目/alpha').attributes('data-depth')).toBe('2');
  });

  it('每个标签显示自己的引用计数；「未分类」用篇数单位（PRD §5.3.6.1 计数口径④）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    expect(findRow(w, '项目').get('[data-testid="tag-count"]').text()).toBe('3 次');
    expect(findRow(w, '随笔').get('[data-testid="tag-count"]').text()).toBe('9 次');
    expect(findRow(w, '').get('[data-testid="tag-count"]').text()).toBe('4 篇');
  });

  it('父节点不显示"子孙合计"（祖先计数已含子孙，相加即重复计数）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    // 项目是 3（自身行数，已含 alpha 的 2 与 beta 的 1）—— 不是 3+2+1=6
    expect(findRow(w, '项目').get('[data-testid="tag-count"]').text()).not.toContain('6');
  });

  it('只有分支带"分支"徽标、叶子不带（is_leaf 仅最末级为真）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    expect(findRow(w, '项目').find('[data-testid="tag-branch-badge"]').exists()).toBe(true);
    expect(findRow(w, '项目/alpha').find('[data-testid="tag-branch-badge"]').exists()).toBe(false);
  });

  it('「未分类」条目排在最后，并带专属徽标（FR-TAG-13）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    const rows = w.findAll('[data-testid="tag-tree-row"]');
    expect(rows.at(-1)?.attributes('data-norm')).toBe('');
    expect(findRow(w, '').find('[data-testid="tag-unclassified-badge"]').exists()).toBe(true);
    expect(w.get('[data-testid="tag-total"]').text()).toContain('未分类 4 篇');
  });

  it('折叠一个分支会让它的子孙行消失，再点一次恢复', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    expect(w.findAll('[data-testid="tag-tree-row"]')).toHaveLength(5);
    await findRow(w, '项目').get('[data-testid="tag-toggle"]').trigger('click');
    // 子孙行整枝消失（折叠只影响渲染，不改数据）
    expect(w.findAll('[data-testid="tag-tree-row"]')).toHaveLength(3);
    expect(w.findAll('[data-testid="tag-tree-row"]').some((r) => r.attributes('data-norm') === '项目/alpha')).toBe(false);
    await findRow(w, '项目').get('[data-testid="tag-toggle"]').trigger('click');
    expect(w.findAll('[data-testid="tag-tree-row"]')).toHaveLength(5);
  });
});

describe('标签面板：点击看笔记（FR-TAG-03、AC-TAG-03）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.tree.mockReset().mockResolvedValue(treeFixture());
    hoisted.notes.mockReset().mockResolvedValue([
      { relPath: 'a.md', name: 'a.md', mtimeMs: 1 },
      { relPath: 'b.md', name: 'b.md', mtimeMs: 2 },
    ]);
  });

  it('点标签 ⇒ 用它的 norm 查该标签下的笔记，并显示篇数（不是 refCount 的行数）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    await findRow(w, '随笔').get('[data-testid="tag-node"]').trigger('click');
    await flushPromises();
    expect(hoisted.notes).toHaveBeenCalledWith('随笔', false);
    expect(w.get('[data-testid="tag-notes-title"]').text()).toContain('共 2 篇');
  });

  it('点「未分类」用**空串**查询（它是合法选中，不是"没选"）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    await findRow(w, '').get('[data-testid="tag-node"]').trigger('click');
    await flushPromises();
    expect(hoisted.notes).toHaveBeenCalledWith('', false);
    expect(w.get('[data-testid="tag-notes-title"]').text()).toContain('未分类');
  });

  it('点笔记把 relPath 交给上层（导航不在面板里）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    await findRow(w, '随笔').get('[data-testid="tag-node"]').trigger('click');
    await flushPromises();
    const buttons = w.findAll('[data-testid="tag-note-item"]');
    await buttons[1]?.trigger('click');
    expect(w.emitted('open')?.[0]).toEqual(['b.md']);
  });

  it('没有选中时给提示而不是空列表', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    expect(w.find('[data-testid="tag-notes-hint"]').exists()).toBe(true);
    expect(hoisted.notes).not.toHaveBeenCalled();
  });

  it('空列表给提示（不假装"标签坏了"）：点进去没有笔记时说明可能是孤儿标签', async () => {
    hoisted.notes.mockResolvedValue([]);
    const w = mount(TagPanel);
    await flushPromises();
    await findRow(w, '随笔').get('[data-testid="tag-node"]').trigger('click');
    await flushPromises();
    expect(w.get('[data-testid="tag-notes-empty"]').text()).toContain('没有笔记');
  });
});

describe('标签面板：排序与过滤（FR-TAG-04）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.tree.mockReset().mockResolvedValue(treeFixture());
    hoisted.notes.mockReset().mockResolvedValue([]);
  });

  it('缺省按引用计数（随笔 9 在项目 3 之前），切到名称会重排', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    const order = () => w.findAll('[data-testid="tag-tree-row"]').map((r) => r.attributes('data-norm'));
    expect(order().slice(0, 2)).toEqual(['随笔', '项目']);
    await w.get('[data-testid="tag-sort"]').setValue('name');
    // 名称排序后仍是"未分类最后"，但前两个的顺序由 display 决定（本条不断言中文 collation）
    expect(order().at(-1)).toBe('');
  });

  it('面板内过滤：命中子孙时保留祖先链；没命中给出说明', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    await w.get('[data-testid="tag-filter"]').setValue('alpha');
    const kept = w.findAll('[data-testid="tag-tree-row"]').map((r) => r.attributes('data-norm'));
    expect(kept).toEqual(['项目', '项目/alpha']);
    await w.get('[data-testid="tag-filter"]').setValue('不存在的标签');
    expect(w.findAll('[data-testid="tag-tree-row"]')).toHaveLength(0);
    expect(w.get('[data-testid="tag-filter-empty"]').text()).toContain('没有匹配');
  });
});

describe('标签面板：include_children 开关必须**如实标注**当前是 no-op（勘误 D-40②）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.tree.mockReset().mockResolvedValue(treeFixture());
    hoisted.notes.mockReset().mockResolvedValue([]);
  });

  it('开关保留，并常驻一行说明"祖先登记使该开关无差别（见 D-40②）"', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    // ① 开关本身在（FR-TAG-03 要求"可切换是否含子级"）
    expect(w.find('[data-testid="tag-include-children"]').exists()).toBe(true);
    // ② 钩子旁边必须有说明，且写明它是 no-op 及原因（不许假装它生效）
    const hint = w.get('[data-testid="tag-include-children-hint"]').text();
    expect(hint).toContain('无差别');
    expect(hint).toContain('祖先');
    expect(hint).toContain('D-40②');
  });

  it('勾选后**如实**把 true 交给后端（no-op 是后端祖先登记造成的，不是 UI 不传）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    await findRow(w, '项目').get('[data-testid="tag-node"]').trigger('click');
    await flushPromises();
    expect(hoisted.notes).toHaveBeenLastCalledWith('项目', false);
    await w.get('[data-testid="tag-include-children"]').setValue(true);
    await flushPromises();
    expect(hoisted.notes).toHaveBeenLastCalledWith('项目', true);
  });
});

describe('标签面板：错误与关闭', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.tree.mockReset().mockResolvedValue(treeFixture());
    hoisted.notes.mockReset().mockResolvedValue([]);
  });

  it('查询失败时面板显示可见错误（R-15），卸载时退订（不留悬挂订阅）', async () => {
    hoisted.tree.mockRejectedValue(new Error('boom'));
    const w = mount(TagPanel);
    await flushPromises();
    expect(w.get('[data-testid="tag-error"]').text()).toContain('boom');
    const store = useTagsStore();
    const dispose = vi.spyOn(store, 'dispose');
    w.unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it('关闭按钮把事件交给上层（面板开关的真身在 store / 布局里）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    await w.get('[data-testid="tag-panel-close"]').trigger('click');
    expect(w.emitted('close')).toBeTruthy();
  });

  it('「刷新」重新查索引库（FR-TAG-12 的**重算**入口是既有的 index_rebuild，不在面板里）', async () => {
    const w = mount(TagPanel);
    await flushPromises();
    const before = hoisted.tree.mock.calls.length;
    await w.get('[data-testid="tag-reload"]').trigger('click');
    await flushPromises();
    expect(hoisted.tree.mock.calls.length).toBe(before + 1);
  });
});

describe('标签重命名（FR-TAG-07/08、AC-TAG-02）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.renamePreview.mockReset();
    hoisted.renameApply.mockReset();
  });

  it('两步确认：先只读预览（文案说的是"处标签"），确认后才执行', async () => {
    hoisted.renamePreview.mockResolvedValue({
      previewId: 'tp1', fromRef: 'old/name', toRef: 'new/name', fileCount: 80, spanCount: 80,
      skipped: [], edits: [{ relPath: 'a.md', hits: 1, occurrences: [{ line: 3, before: '#old/name', after: '#new/name' }] }],
      createdAtMs: 1,
    });

    hoisted.renameApply.mockResolvedValue({ operationId: 'op-t', fileCount: 80, spanCount: 80 });
    const w = mount(TagRenameDialog, { props: { fromTag: 'old/name' } });
    await w.get('[data-testid="rewrite-from"]').setValue('old/name');
    await w.get('[data-testid="rewrite-to"]').setValue('new/name');
    await w.get('[data-testid="rewrite-preview"]').trigger('click');
    await flushPromises();
    expect(hoisted.renamePreview).toHaveBeenCalledWith('old/name', 'new/name');
    expect(w.get('[data-testid="rewrite-preview-summary"]').text()).toContain('将修改 80 个文件中的 80 处标签');
    expect(hoisted.renameApply).not.toHaveBeenCalled();
    expect(w.get('[data-testid="tag-rename-preview-diff"]').text()).toContain('#old/name');

    await w.get('[data-testid="rewrite-apply"]').trigger('click');
    await flushPromises();
    expect(hoisted.renameApply).toHaveBeenCalledWith('old/name', 'new/name', 'tp1');
    expect(w.get('[data-testid="rewrite-applied"]').text()).toContain('已改写 80 个文件 / 80 处标签');
    expect(w.emitted('renamed')?.[0]?.[0]).toEqual({ operationId: 'op-t', fileCount: 80, spanCount: 80 });
  });

  it('已选中标签时给出提示（层级子标签的前缀会一并更新）', async () => {
    const w = mount(TagRenameDialog, { props: { fromTag: '项目' } });
    expect(w.get('[data-testid="tag-rename-from-hint"]').text()).toContain('项目');
  });

  it('层级冲突（FR-TAG-08）由后端拒绝：原样透出原因，且**不调** apply', async () => {
    hoisted.renamePreview.mockRejectedValue({ code: 'E_INVALID_FILENAME', message: '层级冲突：新名是自身的子孙' });
    const w = mount(TagRenameDialog);
    await w.get('[data-testid="rewrite-from"]').setValue('a');
    await w.get('[data-testid="rewrite-to"]').setValue('a/b');
    await w.get('[data-testid="rewrite-preview"]').trigger('click');
    await flushPromises();
    expect(w.get('[data-testid="rewrite-error"]').text()).toContain('新名称不合法');
    expect(w.get('[data-testid="rewrite-error"]').text()).toContain('层级冲突');
    expect(hoisted.renameApply).not.toHaveBeenCalled();
  });

  it('预览过期（E_PREVIEW_EXPIRED）⇒ 提示重新预览，并**锁住**确认按钮', async () => {
    hoisted.renamePreview.mockResolvedValue({
      previewId: 'tp2', fromRef: 'a', toRef: 'b', fileCount: 1, spanCount: 1,
      skipped: [], edits: [], createdAtMs: 1,
    });
    hoisted.renameApply.mockRejectedValue({ code: 'E_PREVIEW_EXPIRED', message: '预览已过期' });
    const w = mount(TagRenameDialog);
    await w.get('[data-testid="rewrite-from"]').setValue('a');
    await w.get('[data-testid="rewrite-to"]').setValue('b');
    await w.get('[data-testid="rewrite-preview"]').trigger('click');
    await flushPromises();
    await w.get('[data-testid="rewrite-apply"]').trigger('click');
    await flushPromises();
    expect(w.get('[data-testid="rewrite-error"]').text()).toContain('预览已过期');
    expect(w.get('[data-testid="rewrite-error"]').text()).toContain('重新点击"预览"');
    // `get` 本身找不到就会抛；这里断言它**渲染出来了**（用 text 而不是 exists —— get 的返回类型上刻意去掉了它）
    expect(w.get('[data-testid="tag-rename-repreview"]').text()).toContain('重新预览');
    // 二次确认按钮被锁住：不让用户一直点一个注定失败的按钮
    expect(w.get('[data-testid="rewrite-apply"]').attributes('disabled')).toBeDefined();
  });
});
