import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/commands', () => ({ noteRead: vi.fn(), noteWrite: vi.fn() }));

import { noteRead, noteWrite } from '@core/ipc/commands';
import {
  AUTOSAVE_DEFAULT_MS,
  AUTOSAVE_MAX_MS,
  AUTOSAVE_MIN_MS,
  useEditorStore,
} from '@features/editor';

const mRead = vi.mocked(noteRead);
const mWrite = vi.mocked(noteWrite);

function note(content: string, mtimeMs = 100) {
  return { relPath: 'a.md', content, mtimeMs, sizeBytes: content.length };
}

describe('useEditorStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
    vi.useFakeTimers();
    mRead.mockResolvedValue(note('原始内容'));
    mWrite.mockResolvedValue({ relPath: 'a.md', newMtime: 200 });
  });
  afterEach(() => vi.useRealTimers());

  it('打开笔记：读盘一次，重复打开只激活不重读', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    await store.openNote('a.md');
    expect(mRead).toHaveBeenCalledTimes(1);
    expect(store.buffers).toHaveLength(1);
    expect(store.activeRelPath).toBe('a.md');
    expect(store.isDirty('a.md')).toBe(false);
  });

  it('AC-EDITOR-02：输入期间不写盘，停止 1 秒后才写一次', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    store.updateContent('a.md', '改了一');
    store.updateContent('a.md', '改了二');
    expect(store.isDirty('a.md')).toBe(true);
    expect(mWrite).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DEFAULT_MS - 10);
    expect(mWrite).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(20);
    expect(mWrite).toHaveBeenCalledTimes(1);
    expect(mWrite).toHaveBeenCalledWith('a.md', '改了二', 100);
    expect(store.isDirty('a.md')).toBe(false);
  });

  it('FR-EDITOR-31：立即保存会清掉防抖队列（只写一次）', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    store.updateContent('a.md', '内容');
    await store.save('a.md');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DEFAULT_MS * 3);
    expect(mWrite).toHaveBeenCalledTimes(1);
  });

  it('未变脏时保存是幂等的', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    await store.save('a.md');
    expect(mWrite).not.toHaveBeenCalled();
  });

  it('FR-EDITOR-34：写冲突进入冲突态，三选项均可处理', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    store.updateContent('a.md', '我的版本');
    mWrite.mockRejectedValueOnce({ code: 'E_WRITE_CONFLICT', message: '文件已被外部修改' });
    await store.save('a.md');
    const buffer = store.buffers[0];
    expect(buffer.conflict).toBe(true);
    expect(buffer.error).toBe('文件已被外部修改');

    mRead.mockResolvedValueOnce(note('外部版本', 300));
    await store.resolveConflict('a.md', 'loadExternal');
    expect(buffer.content).toBe('外部版本');
    expect(buffer.conflict).toBe(false);
    expect(store.isDirty('a.md')).toBe(false);

    store.updateContent('a.md', '我的修订');
    mRead.mockResolvedValueOnce(note('外部又改了', 400));
    await store.resolveConflict('a.md', 'keepMine');
    expect(mWrite).toHaveBeenLastCalledWith('a.md', '我的修订', 400);

    mRead.mockResolvedValueOnce(note('外部\n公共', 500));
    buffer.content = '我的\n公共';
    await store.resolveConflict('a.md', 'viewDiff');
    expect(buffer.diff?.some((l) => l.type === 'removed' && l.text === '外部')).toBe(true);
    expect(buffer.diff?.some((l) => l.type === 'added' && l.text === '我的')).toBe(true);
    expect(buffer.diff?.some((l) => l.type === 'same' && l.text === '公共')).toBe(true);
  });

  it('AC-FILE-05：跟随重命名保留未保存内容，并写到新路径', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    store.updateContent('a.md', '未保存的编辑');
    store.followRename('a.md', 'dir/b.md');
    expect(store.buffers[0].relPath).toBe('dir/b.md');
    expect(store.activeRelPath).toBe('dir/b.md');
    expect(store.buffers[0].content).toBe('未保存的编辑');
    expect(store.isDirty('dir/b.md')).toBe(true);
    await store.save('dir/b.md');
    expect(mWrite).toHaveBeenLastCalledWith('dir/b.md', '未保存的编辑', -1);
  });

  it('FR-EDITOR-35：标签重排（越界索引忽略）', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    await store.openNote('b.md');
    await store.openNote('c.md');
    store.reorderTab(0, 2);
    expect(store.buffers.map((b) => b.relPath)).toEqual(['b.md', 'c.md', 'a.md']);
    store.reorderTab(2, 0);
    expect(store.buffers.map((b) => b.relPath)).toEqual(['a.md', 'b.md', 'c.md']);
    store.reorderTab(0, 9);
    expect(store.buffers.map((b) => b.relPath)).toEqual(['a.md', 'b.md', 'c.md']);
  });

  it('FR-EDITOR-35：关闭其他 / 关闭右侧，并清理其自动保存定时器', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    await store.openNote('b.md');
    await store.openNote('c.md');
    store.updateContent('b.md', 'B2');
    store.updateContent('c.md', 'C2');
    expect(store.dirtyAmong(['b.md', 'c.md'])).toEqual(['b.md', 'c.md']);

    store.closeToTheRight('a.md');
    expect(store.buffers.map((x) => x.relPath)).toEqual(['a.md']);
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DEFAULT_MS * 3);
    expect(mWrite).not.toHaveBeenCalled();

    await store.openNote('b.md');
    store.closeOthers('b.md');
    expect(store.buffers.map((x) => x.relPath)).toEqual(['b.md']);
    expect(store.activeRelPath).toBe('b.md');
  });
  it('防抖时长夹取在 0.5–5 秒（FR-EDITOR-30）', () => {
    const store = useEditorStore();
    store.setAutosaveDelay(100);
    expect(store.autosaveDelayMs).toBe(AUTOSAVE_MIN_MS);
    store.setAutosaveDelay(9999);
    expect(store.autosaveDelayMs).toBe(AUTOSAVE_MAX_MS);
    store.setAutosaveDelay(2000);
    expect(store.autosaveDelayMs).toBe(2000);
  });

  it('关闭标签会取消待执行的自动保存', async () => {
    const store = useEditorStore();
    await store.openNote('a.md');
    store.updateContent('a.md', '内容');
    store.closeNote('a.md');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DEFAULT_MS * 3);
    expect(mWrite).not.toHaveBeenCalled();
    expect(store.buffers).toHaveLength(0);
    expect(store.activeRelPath).toBeNull();
  });

  it('saveAll 保存所有脏缓冲区', async () => {
    const store = useEditorStore();
    mRead.mockResolvedValueOnce(note('x'));
    await store.openNote('a.md');
    mRead.mockResolvedValueOnce(note('y'));
    await store.openNote('b.md');
    store.updateContent('a.md', 'A2');
    store.updateContent('b.md', 'B2');
    await store.saveAll();
    expect(mWrite).toHaveBeenCalledTimes(2);
    expect(store.hasUnsaved()).toBe(false);
  });
});
