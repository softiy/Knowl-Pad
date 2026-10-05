import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import { noteRead, noteWrite } from '@core/ipc/commands';
import { diffLines, type DiffLine } from '@core/utils/diff';

export type EditorMode = 'edit' | 'split' | 'read';
export type ConflictChoice = 'loadExternal' | 'keepMine' | 'viewDiff';

export interface EditorBuffer {
  relPath: string;
  /** 当前编辑内容 */
  content: string;
  /** 上次加载/保存的内容（脏判定基线） */
  savedContent: string;
  /** 冲突检测基线（note_write 的 baseMtime） */
  mtimeMs: number;
  saving: boolean;
  error: string | null;
  /** 磁盘文件在打开期间被外部修改（FR-EDITOR-34） */
  conflict: boolean;
  /** 计数器：仅当 **store 侧整体替换**了内容时递增（如「加载外部版本」）。
   *  视图据此显式回灌内核——**不能**监听 content 本身，否则内核自己的编辑会被回灌并清空其撤销栈（AC-EDITOR-06）。 */
  replacedAt: number;
  /** 「查看差异」的结果（仅在与外部版本比较后填充） */
  diff: DiffLine[] | null;
}

/** 自动保存防抖默认值（FR-EDITOR-30：默认 1s，可配 0.5–5s）。 */
export const AUTOSAVE_DEFAULT_MS = 1000;
export const AUTOSAVE_MIN_MS = 500;
export const AUTOSAVE_MAX_MS = 5000;

function toBuffer(relPath: string, content: string, mtimeMs: number): EditorBuffer {
  return {
    relPath,
    content,
    savedContent: content,
    mtimeMs,
    saving: false,
    error: null,
    conflict: false,
    replacedAt: 0,
    diff: null,
  };
}

/**
 * 编辑器状态（缓冲区 / 自动保存 / 冲突）。
 *
 * 职责边界：本 store 只管**内容与落盘**；渲染走 §7.2 的净化管线（`MarkdownView`），
 * 内核交互走 `KpEditorAdapter`（§7.4）——换内核不影响这里。
 */
export const useEditorStore = defineStore('editor', () => {
  const buffers = ref<EditorBuffer[]>([]);
  const activeRelPath = ref<string | null>(null);
  const mode = ref<EditorMode>('edit');
  const autosaveDelayMs = ref(AUTOSAVE_DEFAULT_MS);
  /** 每个缓冲区的自动保存定时器（关闭/卸载时必须清掉，ED-01 的同源约束） */
  const autosaveTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function find(relPath: string): EditorBuffer | undefined {
    return buffers.value.find((b) => b.relPath === relPath);
  }

  function cancelAutosave(relPath: string): void {
    const timer = autosaveTimers.get(relPath);
    if (timer !== undefined) {
      clearTimeout(timer);
      autosaveTimers.delete(relPath);
    }
  }

  /** 打开笔记（已打开则仅激活，不重复读盘、不丢未保存内容）。 */
  async function openNote(relPath: string): Promise<void> {
    const existing = find(relPath);
    if (existing) {
      activeRelPath.value = relPath;
      return;
    }
    const note = await noteRead(relPath);
    buffers.value = [...buffers.value, toBuffer(relPath, note.content, note.mtimeMs)];
    activeRelPath.value = relPath;
  }

  function activate(relPath: string): void {
    if (find(relPath)) activeRelPath.value = relPath;
  }

  /** 关闭标签（脏内容由调用方先确认；这里只负责清定时器与移除）。 */
  function closeNote(relPath: string): void {
    cancelAutosave(relPath);
    buffers.value = buffers.value.filter((b) => b.relPath !== relPath);
    if (activeRelPath.value === relPath) {
      activeRelPath.value = buffers.value.at(-1)?.relPath ?? null;
    }
  }

  function isDirty(relPath: string): boolean {
    const buffer = find(relPath);
    return buffer ? buffer.content !== buffer.savedContent : false;
  }

  function hasUnsaved(): boolean {
    return buffers.value.some((b) => b.content !== b.savedContent);
  }

  /** 内容变更：更新缓冲、安排防抖保存（FR-EDITOR-30）。 */
  function updateContent(relPath: string, content: string): void {
    const buffer = find(relPath);
    if (!buffer || buffer.content === content) return;
    buffer.content = content;
    buffer.diff = null;
    cancelAutosave(relPath);
    autosaveTimers.set(
      relPath,
      setTimeout(() => {
        autosaveTimers.delete(relPath);
        void save(relPath);
      }, autosaveDelayMs.value),
    );
  }

  /** 立即保存（Ctrl/Cmd+S，FR-EDITOR-31）：清掉防抖队列后写盘。 */
  async function save(relPath: string): Promise<void> {
    const buffer = find(relPath);
    if (!buffer) return;
    cancelAutosave(relPath);
    if (buffer.content === buffer.savedContent && !buffer.conflict) return; // 幂等
    buffer.saving = true;
    buffer.error = null;
    try {
      const result = await noteWrite(relPath, buffer.content, buffer.mtimeMs);
      buffer.mtimeMs = result.newMtime;
      buffer.savedContent = buffer.content;
      buffer.conflict = false;
      buffer.diff = null;
    } catch (err) {
      const kpError = asKpError(err);
      // 写冲突 = 磁盘在打开期间被外部修改（FR-EDITOR-34）：进入冲突态，交由用户选择
      buffer.conflict = kpError.code === 'E_WRITE_CONFLICT';
      buffer.error = kpError.message;
    } finally {
      buffer.saving = false;
    }
  }

  /** 保存全部（切换 Vault / 关闭窗口前调用）。 */
  async function saveAll(): Promise<void> {
    await Promise.all(buffers.value.map((b) => save(b.relPath)));
  }

  /** 冲突处理三选项（FR-EDITOR-34）：加载外部 / 保留我的并覆盖 / 查看差异。 */
  async function resolveConflict(relPath: string, choice: ConflictChoice): Promise<void> {
    const buffer = find(relPath);
    if (!buffer) return;
    if (choice === 'loadExternal') {
      const note = await noteRead(relPath);
      buffer.content = note.content;
      buffer.savedContent = note.content;
      buffer.mtimeMs = note.mtimeMs;
      buffer.conflict = false;
      buffer.error = null;
      buffer.replacedAt += 1; // 显式通知视图回灌内核
      return;
    }
    if (choice === 'keepMine') {
      // 基线刷新为磁盘当前版本，再强制覆盖（用户已明确选择「保留我的」）
      const note = await noteRead(relPath);
      buffer.mtimeMs = note.mtimeMs;
      buffer.savedContent = note.content;
      await save(relPath);
      return;
    }
    // 查看差异：拉取外部版本与当前内容做行级比较
    const note = await noteRead(relPath);
    buffer.diff = diffLines(note.content, buffer.content).lines;
  }

  /** 编辑器跟随重命名（FR-FILE-28 / AC-FILE-05）：不丢未保存内容、不报「文件不存在」。 */
  function followRename(oldRelPath: string, newRelPath: string): void {
    const buffer = find(oldRelPath);
    if (!buffer) return;
    cancelAutosave(oldRelPath);
    buffer.relPath = newRelPath;
    // 重命名后磁盘 mtime 变化，基线置空由下次保存刷新（内容与 dirty 状态保持不变）
    buffer.mtimeMs = -1;
    if (activeRelPath.value === oldRelPath) activeRelPath.value = newRelPath;
  }

  /** 标签重排（FR-EDITOR-35 的拖拽排序）：越界索引忽略，不做部分移动。 */
  function reorderTab(fromIndex: number, toIndex: number): void {
    const list = [...buffers.value];
    if (fromIndex === toIndex) return;
    if (fromIndex < 0 || fromIndex >= list.length || toIndex < 0 || toIndex >= list.length) return;
    const [moved] = list.splice(fromIndex, 1);
    list.splice(toIndex, 0, moved);
    buffers.value = list;
  }

  /** 关闭其他标签（FR-EDITOR-35）；未保存内容由调用方先确认。 */
  function closeOthers(relPath: string): void {
    if (!find(relPath)) return;
    for (const buffer of buffers.value) {
      if (buffer.relPath !== relPath) cancelAutosave(buffer.relPath);
    }
    buffers.value = buffers.value.filter((b) => b.relPath === relPath);
    activeRelPath.value = relPath;
  }

  /** 关闭右侧标签（FR-EDITOR-35）；未保存内容由调用方先确认。 */
  function closeToTheRight(relPath: string): void {
    const index = buffers.value.findIndex((b) => b.relPath === relPath);
    if (index < 0) return;
    for (const buffer of buffers.value.slice(index + 1)) cancelAutosave(buffer.relPath);
    buffers.value = buffers.value.slice(0, index + 1);
    if (!buffers.value.some((b) => b.relPath === activeRelPath.value)) {
      activeRelPath.value = relPath;
    }
  }

  /** 给定标签集合里的未保存项（供批量关闭前一次性确认，FR-EDITOR-32）。 */
  function dirtyAmong(relPaths: string[]): string[] {
    return relPaths.filter((relPath) => isDirty(relPath));
  }

  /** 自动保存防抖时长（FR-EDITOR-30：0.5–5s，越界即夹取）。 */
  function setAutosaveDelay(ms: number): void {
    autosaveDelayMs.value = Math.min(AUTOSAVE_MAX_MS, Math.max(AUTOSAVE_MIN_MS, Math.round(ms)));
  }

  function setMode(next: EditorMode): void {
    mode.value = next;
  }

  /** 关闭全部（清定时器，避免卸载后仍触发保存）。 */
  function closeAll(): void {
    for (const buffer of buffers.value) cancelAutosave(buffer.relPath);
    buffers.value = [];
    activeRelPath.value = null;
  }

  return {
    buffers,
    activeRelPath,
    mode,
    autosaveDelayMs,
    openNote,
    activate,
    closeNote,
    closeAll,
    isDirty,
    hasUnsaved,
    updateContent,
    save,
    saveAll,
    resolveConflict,
    followRename,
    reorderTab,
    closeOthers,
    closeToTheRight,
    dirtyAmong,
    setAutosaveDelay,
    setMode,
  };
});
