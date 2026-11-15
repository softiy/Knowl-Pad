import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({
  noteCreate: vi.fn(),
  folderCreate: vi.fn(),
  fileRename: vi.fn(),
  fileDelete: vi.fn(),
  fileValidateName: vi.fn(async () => ({ valid: true })),
}));
vi.mock('@features/file-tree/stores/fileTree', () => ({
  useFileTreeStore: () => ({
    refresh: vi.fn(),
    nodeAdded: vi.fn(async () => {}),
    pathMoved: vi.fn(),
    pathRemoved: vi.fn(),
  }),
}));
// FR-FILE-32：删除确认里的引用数来自既有反链查询（替身，用例按需给值）
vi.mock('@core/ipc/linkCommands', () => ({ linkBacklinks: vi.fn(async () => []) }));
vi.mock('@features/editor', () => ({
  useEditorStore: () => ({ openNote: vi.fn(), closeNote: vi.fn(), followRename: vi.fn() }),
}));

import { linkBacklinks } from '@core/ipc/linkCommands';
import { FileOpsDialog, useFileOpsStore } from '@features/file-tree';

const mBacklinks = vi.mocked(linkBacklinks);

/**
 * 挂载对话框。`attachTo` 只在需要断言 document.activeElement 的用例里传：
 * jsdom 的 focus() 对**未挂到文档**的元素不生效（activeElement 会停在 body）。
 */
function mountDialog(attachTo?: HTMLElement) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const ops = useFileOpsStore();
  const wrapper = mount(FileOpsDialog, {
    global: { plugins: [pinia] },
    ...(attachTo === undefined ? {} : { attachTo }),
  });
  return { wrapper, ops };
}

describe('FileOpsDialog', () => {
  beforeEach(() => vi.resetAllMocks());

  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('无操作时不渲染', () => {
    const { wrapper } = mountDialog();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(false);
  });

  it('新建笔记：显示名称输入与确定/取消', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openNewNote('dir');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-dialog"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(true);
    expect(wrapper.text()).toContain('新建笔记');
  });

  it('AC-FILE-03：非法名称显示具体原因', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openNewFolder('');
    ops.dialog!.validation = { valid: false, reason: '名称以点号结尾' };
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-validation"]').text()).toBe('名称以点号结尾');
  });

  it('AC-FILE-04：冲突时展示三选项并回传选择', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openNewNote('');
    const resolve = vi.spyOn(ops, 'resolveConflict').mockResolvedValue(undefined);
    ops.dialog!.conflict = true;
    ops.dialog!.error = '文件已存在';
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-conflict"]').text()).toContain('自动备份');
    await wrapper.find('[data-testid="conflict-overwrite"]').trigger('click');
    expect(resolve).toHaveBeenCalledWith('overwrite');
    await wrapper.find('[data-testid="conflict-rename-new"]').trigger('click');
    expect(resolve).toHaveBeenCalledWith('renameNew');
  });

  it('FR-FILE-32 / AC-FILE-11：N>0 时显示「该笔记被 N 处引用」并把按钮改成「仍然删除」', async () => {
    mBacklinks.mockResolvedValue([
      { srcRelPath: 'a.md', srcName: 'a', linkCount: 4, embedCount: 0, items: [] },
      { srcRelPath: 'b.md', srcName: 'b', linkCount: 1, embedCount: 0, items: [] },
    ]);
    const { wrapper, ops } = mountDialog();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(5));
    await wrapper.vm.$nextTick();
    const warn = wrapper.find('[data-testid="delete-references"]');
    expect(warn.exists()).toBe(true);
    // 文案逐字对齐 AC-FILE-11
    expect(warn.text()).toContain('该笔记被 5 处引用，删除后这些链接将变为悬空链接');
    expect(wrapper.find('[data-testid="fileops-submit"]').text()).toBe('仍然删除');
  });

  it('FR-FILE-32：N=0 时**不**出现提示、按钮保持「确定」（不制造噪声）', async () => {
    mBacklinks.mockResolvedValue([]);
    const { wrapper, ops } = mountDialog();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceCount).toBe(0));
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="delete-references"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="fileops-submit"]').text()).toBe('确定');
  });

  it('R-15：统计失败时如实提示「无法统计引用」，不伪装成 0 处', async () => {
    mBacklinks.mockRejectedValue({ code: 'E_VAULT_NOT_OPEN', message: '知识库未打开' });
    const { wrapper, ops } = mountDialog();
    ops.openDelete('A.md');
    await vi.waitFor(() => expect(ops.dialog?.referenceError).toBe('知识库未打开'));
    await wrapper.vm.$nextTick();
    const err = wrapper.find('[data-testid="delete-references-error"]');
    expect(err.exists()).toBe(true);
    expect(err.text()).toContain('无法统计引用');
    expect(wrapper.find('[data-testid="delete-references"]').exists()).toBe(false);
  });

  it('DEBT-19：移动到… 显示**目标路径**输入与说明（file_move 的 UI 入口）', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openMove('dir/a.md');
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain('移动到');
    expect(wrapper.find('[data-testid="fileops-move-hint"]').exists()).toBe(true);
    const input = wrapper.find('[data-testid="fileops-move-target"]');
    expect(input.exists()).toBe(true);
    expect((input.element as HTMLInputElement).value).toBe('dir/a.md');
    // 移动用的是路径输入，不是"名称"输入（两者语义不同，不能混用同一个 testid）
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(false);
  });

  it('删除：提示移入回收站（不删磁盘内容）且不显示名称输入', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openDelete('dir/a.md');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="fileops-delete-hint"]').text()).toContain('回收站');
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(false);
  });
});

describe('FileOpsDialog：打开即聚焦 + 新建笔记默认 .md（M5 冒烟 5.6）', () => {
  beforeEach(() => vi.resetAllMocks());
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('新建笔记：默认名「未命名.md」，输入框自动聚焦且只选中主文件名', async () => {
    const { wrapper, ops } = mountDialog(document.body);
    ops.openNewNote('dir');
    await flushPromises();
    const input = wrapper.find('[data-testid="fileops-name"]').element as HTMLInputElement;
    expect(input.value).toBe('未命名.md');
    expect(document.activeElement).toBe(input);
    // 只选中「未命名」：输入时不会把 .md 一起删掉（5.6 的追加修复靠这里保持可见）
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe('未命名'.length);
    wrapper.unmount();
  });

  it('重命名：聚焦输入框并选中主文件名', async () => {
    const { wrapper, ops } = mountDialog(document.body);
    ops.openRename('dir/a.md');
    await flushPromises();
    const input = wrapper.find('[data-testid="fileops-name"]').element as HTMLInputElement;
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe('a.md');
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 1]);
    wrapper.unmount();
  });

  it('移动到…：聚焦目标路径输入框并整体选中（路径要整段替换）', async () => {
    const { wrapper, ops } = mountDialog(document.body);
    ops.openMove('dir/a.md');
    await flushPromises();
    const input = wrapper.find('[data-testid="fileops-move-target"]').element as HTMLInputElement;
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, input.value.length]);
    wrapper.unmount();
  });

  it('删除确认没有输入框，不抢焦点（焦点留给按钮/已有界面）', async () => {
    const { wrapper, ops } = mountDialog(document.body);
    ops.openDelete('dir/a.md');
    await flushPromises();
    expect(wrapper.find('[data-testid="fileops-name"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it('「与当前相同」以红字显示在对话框内（M5 冒烟 5.4 的拦截提示）', async () => {
    const { wrapper, ops } = mountDialog();
    ops.openMove('dir/a.md');
    await wrapper.vm.$nextTick();
    ops.dialog!.error = '与当前相同，无需移动';
    await wrapper.vm.$nextTick();
    const error = wrapper.find('[data-testid="fileops-error"]');
    expect(error.text()).toContain('与当前相同');
    // 红色样式类（不是灰色提示）：复用既有 .kp-fileops__error
    expect(error.classes()).toContain('kp-fileops__error');
  });
});
