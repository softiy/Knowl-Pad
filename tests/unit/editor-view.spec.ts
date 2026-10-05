import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { flushPromises } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

/** 适配器替身：只验证视图与适配器的交互契约（真实内核的测试在 editor-adapter.spec.ts）。 */
const hoisted = vi.hoisted(() => {
  const adapter = {
    mount: vi.fn(),
    getValue: vi.fn(() => ''),
    setValue: vi.fn(),
    on: vi.fn(),
    insertLink: vi.fn(),
    triggerSuggest: vi.fn(),
    find: vi.fn((): { from: number; to: number; text: string }[] => []),
    replace: vi.fn(() => ({ replaced: 0 })),
    destroy: vi.fn(),
  };
  return { adapter };
});

vi.mock('@features/editor/adapter', () => ({
  createEditorAdapter: () => hoisted.adapter,
  currentEngine: () => 'md-editor-v3',
}));

vi.mock('@core/ipc/commands', () => ({ noteRead: vi.fn(), noteWrite: vi.fn() }));

import { noteRead, noteWrite } from '@core/ipc/commands';
import { EditorView, useEditorStore } from '@features/editor';

const mRead = vi.mocked(noteRead);
const mWrite = vi.mocked(noteWrite);

function note(relPath: string, content: string, mtimeMs = 100) {
  return { relPath, content, mtimeMs, sizeBytes: content.length };
}

async function mountView() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const wrapper = mount(EditorView, { global: { plugins: [pinia] } });
  await flushPromises();
  return { wrapper, store: useEditorStore() };
}

describe('EditorView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mRead.mockResolvedValue(note('a.md', '原始'));
    mWrite.mockResolvedValue({ relPath: 'a.md', newMtime: 200 });
  });

  it('无打开笔记时给出空状态，并已挂载内核', async () => {
    const { wrapper } = await mountView();
    expect(wrapper.find('[data-testid="editor-empty"]').exists()).toBe(true);
    expect(hoisted.adapter.mount).toHaveBeenCalledTimes(1);
  });

  it('FR-EDITOR-04：三态切换（纯编辑 / 分屏 / 纯阅读）', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    // v-show 的契约就是 display 切换：直接断言内联样式，比 isVisible() 在 jsdom 下更可靠
    const display = (testid: string): string =>
      (wrapper.find(`[data-testid="${testid}"]`).element as HTMLElement).style.display;
    expect(display('editor-host')).not.toBe('none');
    expect(display('editor-preview')).toBe('none');

    await wrapper.find('[data-testid="mode-split"]').trigger('click');
    expect(display('editor-host')).not.toBe('none');
    expect(display('editor-preview')).not.toBe('none');

    await wrapper.find('[data-testid="mode-read"]').trigger('click');
    expect(display('editor-host')).toBe('none');
    expect(display('editor-preview')).not.toBe('none');
  });

  it('切换标签会把内容灌进内核（程序化写入）', async () => {
    const { store } = await mountView();
    mRead.mockResolvedValue(note('a.md', '内容A'));
    await store.openNote('a.md');
    await flushPromises();
    expect(hoisted.adapter.setValue).toHaveBeenCalledWith('内容A');
  });

  it('FR-EDITOR-32：脏标签显示标记，关闭前必须确认', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    expect(wrapper.find('[data-testid="dirty-dot"]').exists()).toBe(false);

    store.updateContent('a.md', '改了');
    await flushPromises();
    expect(wrapper.find('[data-testid="dirty-dot"]').exists()).toBe(true);

    await wrapper.find('[data-testid="tab-close"]').trigger('click');
    expect(wrapper.find('[data-testid="close-confirm"]').exists()).toBe(true);
    expect(store.buffers).toHaveLength(1);

    const discard = wrapper.findAll('[data-testid="close-confirm"] button').find((b) => b.text() === '放弃修改');
    await discard?.trigger('click');
    expect(store.buffers).toHaveLength(0);
  });

  it('FR-EDITOR-32：关闭脏标签时可选「保存并关闭」', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    store.updateContent('a.md', '需要保存的修改');
    await flushPromises();
    await wrapper.find('[data-testid="tab-close"]').trigger('click');
    const saveAndClose = wrapper
      .findAll('[data-testid="close-confirm"] button')
      .find((b) => b.text() === '保存并关闭');
    await saveAndClose?.trigger('click');
    await flushPromises();
    expect(mWrite).toHaveBeenCalledWith('a.md', '需要保存的修改', 100);
    expect(store.buffers).toHaveLength(0);
  });
  it('FR-EDITOR-34：冲突时展示三选项并回调 store', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    const resolve = vi.spyOn(store, 'resolveConflict').mockResolvedValue(undefined);
    store.buffers[0].conflict = true;
    await flushPromises();
    expect(wrapper.find('[data-testid="conflict-bar"]').exists()).toBe(true);

    const buttons = wrapper.findAll('[data-testid="conflict-bar"] button').map((b) => b.text());
    expect(buttons).toContain('加载外部版本');
    expect(buttons).toContain('保留我的版本并覆盖');
    expect(buttons).toContain('查看差异');

    await wrapper.findAll('[data-testid="conflict-bar"] button')[0].trigger('click');
    expect(resolve).toHaveBeenCalledWith('a.md', 'loadExternal');
  });

  it('FR-EDITOR-31：Mod+S 触发立即保存（快捷键走 core/shortcut）', async () => {
    const { store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    const save = vi.spyOn(store, 'save').mockResolvedValue(undefined);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, cancelable: true }));
    expect(save).toHaveBeenCalledWith('a.md');
  });

  it('ED-01：卸载时销毁内核并注销快捷键', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    const save = vi.spyOn(store, 'save').mockResolvedValue(undefined);
    wrapper.unmount();
    expect(hoisted.adapter.destroy).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, cancelable: true }));
    expect(save).not.toHaveBeenCalled();
  });

  it('FR-EDITOR-35：拖拽排序标签', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await store.openNote('b.md');
    await flushPromises();
    await wrapper.find('[data-testid="tab-a.md"]').trigger('dragstart');
    await wrapper.find('[data-testid="tab-b.md"]').trigger('drop');
    expect(store.buffers.map((x) => x.relPath)).toEqual(['b.md', 'a.md']);
  });

  it('FR-EDITOR-35：中键关闭标签', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    await wrapper.find('[data-testid="tab-a.md"]').trigger('auxclick', { button: 1 });
    expect(store.buffers).toHaveLength(0);
  });

  it('FR-EDITOR-35：右键菜单可关闭右侧（未保存先确认）', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await store.openNote('b.md');
    store.updateContent('b.md', '未保存');
    await flushPromises();
    await wrapper.find('[data-testid="tab-a.md"]').trigger('contextmenu');
    const closeRight = wrapper.findAll('[data-testid="tab-menu"] button').find((x) => x.text() === '关闭右侧');
    await closeRight?.trigger('click');
    expect(wrapper.find('[data-testid="close-confirm"]').exists()).toBe(true);
    expect(store.buffers).toHaveLength(2);
    const discard = wrapper.findAll('[data-testid="close-confirm"] button').find((x) => x.text() === '放弃修改');
    await discard?.trigger('click');
    expect(store.buffers.map((x) => x.relPath)).toEqual(['a.md']);
  });

  it('FR-EDITOR-09：查找替换面板统计命中并执行全部替换', async () => {
    const { wrapper, store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    hoisted.adapter.find.mockReturnValue([
      { from: 0, to: 3, text: 'foo' },
      { from: 8, to: 11, text: 'foo' },
    ]);
    await wrapper.find('[data-testid="find-toggle"]').trigger('click');
    expect(wrapper.find('[data-testid="find-panel"]').exists()).toBe(true);
    await wrapper.find('[data-testid="find-query"]').setValue('foo');
    expect(wrapper.find('[data-testid="find-count"]').text()).toContain('2');
    await wrapper.find('[data-testid="find-replacement"]').setValue('bar');
    hoisted.adapter.find.mockReturnValueOnce([
      { from: 0, to: 3, text: 'foo' },
      { from: 8, to: 11, text: 'foo' },
    ]);
    hoisted.adapter.find.mockReturnValue([]);
    await wrapper.find('[data-testid="replace-all"]').trigger('click');
    expect(hoisted.adapter.replace).toHaveBeenCalledWith(
      [
        { from: 0, to: 3, text: 'foo' },
        { from: 8, to: 11, text: 'foo' },
      ],
      'bar',
    );
    expect(wrapper.find('[data-testid="find-count"]').text()).toContain('0');
  });

  it('「加载外部版本」会显式回灌内核（replacedAt 信号）', async () => {
    const { store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    hoisted.adapter.setValue.mockClear();
    mRead.mockResolvedValueOnce(note('a.md', '外部版本', 500));
    await store.resolveConflict('a.md', 'loadExternal');
    await flushPromises();
    expect(hoisted.adapter.setValue).toHaveBeenCalledWith('外部版本');
  });

  it('AC-EDITOR-06（结构保证）：保存不会重置内核内容，撤销栈不会被清空', async () => {
    const { store } = await mountView();
    await store.openNote('a.md');
    await flushPromises();
    hoisted.adapter.setValue.mockClear();
    store.updateContent('a.md', '编辑后');
    await store.save('a.md');
    await flushPromises();
    // 保存只走 note_write，**从不**回灌内核：内核自带的撤销历史因此保持（真机深度由 M8 的正式验证覆盖）
    expect(hoisted.adapter.setValue).not.toHaveBeenCalled();
    expect(mWrite).toHaveBeenCalledWith('a.md', '编辑后', 100);
  });
  it('自动保存时长可配置（FR-EDITOR-30）', async () => {
    const { wrapper, store } = await mountView();
    await wrapper.find('[data-testid="autosave-delay"]').setValue('3000');
    expect(store.autosaveDelayMs).toBe(3000);
  });
});
