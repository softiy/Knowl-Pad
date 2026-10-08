import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

/**
 * 链接面板（FR-LINK-10~22）：
 * 反链分组与**普通/嵌入分开计数**（FR-LINK-14/15）、四类清单的页签计数、
 * 以及改写的**两步确认**（FR-FILE-21 + PRD §5.3.3 的 preview_id 流程）。
 */
const hoisted = vi.hoisted(() => ({
  backlinks: vi.fn(),
  dangling: vi.fn(),
  ambiguous: vi.fn(),
  orphans: vi.fn(),
  preview: vi.fn(),
  apply: vi.fn(),
  rollback: vi.fn(),
}));

vi.mock('@core/ipc/linkCommands', () => ({
  linkBacklinks: hoisted.backlinks,
  linkDanglingList: hoisted.dangling,
  linkAmbiguousList: hoisted.ambiguous,
  linkOrphanList: hoisted.orphans,
  linkRewritePreview: hoisted.preview,
  linkRewriteApply: hoisted.apply,
  linkRewriteRollback: hoisted.rollback,
}));
vi.mock('@core/ipc/events', () => ({
  onIndexCompleted: vi.fn(async () => () => {}),
  onFsCreated: vi.fn(async () => () => {}),
  onFsModified: vi.fn(async () => () => {}),
  onFsRemoved: vi.fn(async () => () => {}),
  onFsRenamed: vi.fn(async () => () => {}),
}));

import LinksPanel from '@features/links/components/LinksPanel.vue';
import RewriteConfirmDialog from '@features/links/components/RewriteConfirmDialog.vue';

describe('链接面板', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.backlinks.mockReset().mockResolvedValue([
      { srcRelPath: 'a.md', srcName: 'a.md', linkCount: 2, embedCount: 1, items: [
        { line: 3, col: 1, linkKind: 'wikilink', alias: '别名', snippet: { lines: [{ line: 3, text: '见 [[b]]', isLinkLine: true }], highlightLine: 3, highlightStart: 2, highlightEnd: 8 } },
        { line: 9, col: 1, linkKind: 'embed', snippet: null },
      ] },
    ]);
    hoisted.dangling.mockReset().mockResolvedValue({ items: [{ targetRef: 'missing', refCount: 4, sourceCount: 2, sampleSources: ['a.md'] }], total: 1 });
    hoisted.ambiguous.mockReset().mockResolvedValue({ items: [{ targetRef: 'note', candidates: ['f1/note.md', 'f2/note.md'], refCount: 10 }], total: 1 });
    hoisted.orphans.mockReset().mockResolvedValue({ items: ['solo.md'], total: 1 });
    hoisted.preview.mockReset();
    hoisted.apply.mockReset();
    hoisted.rollback.mockReset();
  });

  it('四个页签显示各自的计数', async () => {
    const w = mount(LinksPanel, { props: { activeRelPath: 'b.md' } });
    await flushPromises();
    expect(w.get('[data-testid="links-tab-backlinks"]').text()).toContain('反链 2');
    expect(w.get('[data-testid="links-tab-dangling"]').text()).toContain('悬空 1');
    expect(w.get('[data-testid="links-tab-ambiguous"]').text()).toContain('歧义 1');
    expect(w.get('[data-testid="links-tab-orphans"]').text()).toContain('孤立 1');
  });

  it('反链按来源分组，并**分开**显示普通链接与嵌入计数（FR-LINK-14/15）', async () => {
    const w = mount(LinksPanel, { props: { activeRelPath: 'b.md' } });
    await flushPromises();
    expect(w.get('[data-testid="backlink-summary"]').text()).toContain('反链 2 条');
    expect(w.get('[data-testid="backlink-summary"]').text()).toContain('嵌入 1 处');
    expect(w.get('[data-testid="backlink-counts"]').text()).toContain('链接 2');
    expect(w.get('[data-testid="backlink-counts"]').text()).toContain('嵌入 1');
    const items = w.findAll('[data-testid="backlink-item"]');
    expect(items.length).toBe(2);
    await items[0].trigger('click');
    expect(w.emitted('open')?.[0]?.[0]).toEqual({ relPath: 'a.md', line: 3 });
  });

  it('没有打开笔记时给出提示而不是空列表', async () => {
    const w = mount(LinksPanel, { props: {} });
    await flushPromises();
    expect(w.find('[data-testid="links-no-note"]').exists()).toBe(true);
    expect(hoisted.backlinks).not.toHaveBeenCalled();
  });

  it('歧义页签列出候选（逐条指定目标已登记顺延）', async () => {
    const w = mount(LinksPanel, { props: {} });
    await flushPromises();
    await w.get('[data-testid="links-tab-ambiguous"]').trigger('click');
    expect(w.get('[data-testid="ambiguous-item"]').text()).toContain('10 处引用');
    expect(w.get('[data-testid="ambiguous-item"]').text()).toContain('f1/note.md');
    expect(w.find('[data-testid="ambiguous-deferred"]').exists()).toBe(true);
  });
});

describe('改写两步确认（FR-FILE-21 / PRD 的 preview_id）', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    hoisted.preview.mockReset().mockResolvedValue({
      previewId: 'p1', fromRef: 'A', toRef: 'B', fileCount: 2, spanCount: 3,
      edits: [{ relPath: 'a.md', hits: 2 }, { relPath: 'b.md', hits: 1 }], createdAtMs: 1,
    });
    hoisted.apply.mockReset().mockResolvedValue({ operationId: 'op1', fileCount: 2, spanCount: 3 });
    hoisted.rollback.mockReset().mockResolvedValue(undefined);
  });

  it('预览只读展示"将修改 N 个文件中的 M 处"，确认后才执行并可回滚', async () => {
    const w = mount(RewriteConfirmDialog);
    await w.get('[data-testid="rewrite-from"]').setValue('A');
    await w.get('[data-testid="rewrite-to"]').setValue('B');
    await w.get('[data-testid="rewrite-preview"]').trigger('click');
    await flushPromises();
    expect(w.get('[data-testid="rewrite-preview-summary"]').text()).toContain('将修改 2 个文件中的 3 处链接');
    expect(hoisted.apply).not.toHaveBeenCalled();

    await w.get('[data-testid="rewrite-apply"]').trigger('click');
    await flushPromises();
    expect(hoisted.apply).toHaveBeenCalledWith('p1', undefined);
    expect(w.get('[data-testid="rewrite-applied"]').text()).toContain('已改写 2 个文件 / 3 处');

    await w.get('[data-testid="rewrite-rollback"]').trigger('click');
    await flushPromises();
    expect(hoisted.rollback).toHaveBeenCalledWith('op1');
  });
});
