import { resolveConflictWith } from './editor_conflict';
import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import { noteRead, noteWrite } from '@core/ipc/commands';
import { clampAutosaveDelay, persistAutosaveDelay, readAutosaveDelay } from './editor_settings';

// 常量与键名在 `editor_settings.ts`（设置 I/O 与编辑状态机分开，见那里的模块注释）；
// 这里再导出，保证既有引用（`@features/editor` 的 index）不用改。
export {
  AUTOSAVE_DEFAULT_MS,
  AUTOSAVE_MIN_MS,
  AUTOSAVE_MAX_MS,
  PREF_AUTOSAVE_DELAY_MS,
} from './editor_settings';
import type { DiffLine } from '@core/utils/diff';

export type EditorMode = 'edit' | 'split' | 'read';
export type ConflictChoice = 'loadExternal' | 'keepMine' | 'viewDiff';

/**
 * 待定位行（FR-LINK-13）：点击反链/搜索结果后由 store 记下，视图侧消费一次。
 *
 * 之所以放在 store（而不是组件里）：**打开笔记是异步的**，组件可能还没挂上，定位请求不能丢。
 */
export interface RevealRequest {
  relPath: string;
  line: number;
  /** 行内区间（相对该行文本）；只有"跳到命中处"才带（FR-SEARCH-07）。 */
  range?: { start: number; end: number };
}

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
/**
 * 「基线未知」哨兵：重命名等场景下磁盘 mtime 已变、不能再用旧值做冲突检测。
 * ⚠️ **绝不能把这个值发给 Rust**——它按 epoch 毫秒比较，Some(-1) 会永远判为冲突（B1）。
 * 保存时必须以 undefined（即 Rust 的 None）表达「不做基线检测」。
 */
export const MTIME_UNKNOWN = -1;

import { AUTOSAVE_DEFAULT_MS } from './editor_settings';

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
/** 打开笔记失败时的可见原因（审计 F1：此前 rejection 被调用方 void 掉，界面毫无反应）。 */
const openError = ref<string | null>(null);
const pendingReveal = ref<RevealRequest | null>(null);
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
    let note;
    try {
      note = await noteRead(relPath);
    } catch (err) {
      // 文件被外部删除/索引里有幽灵行时，noteRead 会抛。此前调用方用 void 丢弃 →
      // 点一下"什么都没发生"（审计 F1）。这里收敛成可见错误，并按 save 的既有做法处理。
      openError.value = asKpError(err).message;
      return;
    }
    openError.value = null;
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
      const baseMtime = buffer.mtimeMs >= 0 ? buffer.mtimeMs : undefined;
      const result = await noteWrite(relPath, buffer.content, baseMtime);
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
  /** 冲突处理三选项（FR-EDITOR-34）：实现见 editor_conflict.ts（审计 F3 的可见错误也在那里）。 */
  async function resolveConflict(relPath: string, choice: ConflictChoice): Promise<void> {
    await resolveConflictWith(find, save, relPath, choice);
  }

  /** 编辑器跟随重命名（FR-FILE-28 / AC-FILE-05）：不丢未保存内容、不报「文件不存在」。 */
  function followRename(oldRelPath: string, newRelPath: string): void {
    const buffer = find(oldRelPath);
    if (!buffer) return;
    cancelAutosave(oldRelPath);
    buffer.relPath = newRelPath;
    // 重命名后磁盘 mtime 变化，基线置空由下次保存刷新（内容与 dirty 状态保持不变）
    buffer.mtimeMs = MTIME_UNKNOWN;
    if (activeRelPath.value === oldRelPath) activeRelPath.value = newRelPath;
  }

  /**
   * **目录重命名时跟随子树**（M1/M2 复核 major：此前只有精确匹配的 `followRename`，
   * 打开中的 `dir/a.md` 仍指向旧路径，之后的自动保存会以不存在的路径写盘 → 报「文件不存在」，
   * FR-FILE-28 明文禁止）。与 `closeUnder` 对称：按前缀改写所有受影响的标签。
   *
   * 返回改写的标签数（调用方可据此提示）。
   */
  function followRenameUnder(oldRelPath: string, newRelPath: string): number {
    const prefix = oldRelPath + '/';
    let changed = 0;
    for (const buffer of buffers.value) {
      if (buffer.relPath === oldRelPath) {
        cancelAutosave(buffer.relPath);
        buffer.relPath = newRelPath;
        buffer.mtimeMs = MTIME_UNKNOWN;
        if (activeRelPath.value === oldRelPath) activeRelPath.value = newRelPath;
        changed += 1;
      } else if (buffer.relPath.startsWith(prefix)) {
        const rest = buffer.relPath.slice(prefix.length);
        const next = `${newRelPath}/${rest}`;
        cancelAutosave(buffer.relPath);
        if (activeRelPath.value === buffer.relPath) activeRelPath.value = next;
        buffer.relPath = next;
        buffer.mtimeMs = MTIME_UNKNOWN;
        changed += 1;
      }
    }
    return changed;
  }

  /** 关闭某个路径及其**子树**下的全部标签（删除文件夹时用，M1/M2 复核）。 */
  function closeUnder(relPath: string): number {
    const prefix = relPath + '/';
    const targets = buffers.value
      .filter((b) => b.relPath === relPath || b.relPath.startsWith(prefix))
      .map((b) => b.relPath);
    for (const p of targets) closeNote(p);
    return targets.length;
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

  /** 自动保存防抖时长（FR-EDITOR-30：0.5–5s 夹取）；立刻生效并**持久化**（DEBT-30）。 */
  function setAutosaveDelay(ms: number): void {
    const clamped = clampAutosaveDelay(ms);
    autosaveDelayMs.value = clamped;
    persistAutosaveDelay(clamped);
  }

  /**
   * 从全局偏好恢复自动保存防抖时长（DEBT-30）。
   *
   * 读不到或值非法（非数字 / NaN）⇒ **保持默认**：这是非关键设置，不为它弹错误横幅；
   * 但也不静默 —— 原因打到控制台（与 adapter 的降级提示同一口径，R-15）。
   */
  async function loadAutosaveDelay(): Promise<void> {
    // 三种情况的分工见 `editor_settings.ts::readAutosaveDelay` 的注释
    const loaded = await readAutosaveDelay();
    if (loaded !== null) autosaveDelayMs.value = loaded;
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

  /**
   * 打开来源笔记并请求定位（FR-LINK-13 的行级；FR-SEARCH-07 的列级命中）。
   *
   * `range` 只在传了才写进请求对象：既有用例断言的是 `{relPath, line}` 这一形状，
   * 不传时保持原样（不引入多余的 `range: undefined`）。
   */
  async function reveal(
    relPath: string,
    line: number,
    range?: { start: number; end: number },
  ): Promise<void> {
    await openNote(relPath);
    pendingReveal.value = range ? { relPath, line, range } : { relPath, line };
  }

  /** 视图消费完定位请求后清除（避免重复定位）。 */
  function clearReveal(): void {
    pendingReveal.value = null;
  }

  return {
    pendingReveal,
    reveal,
    clearReveal,
    buffers,
    activeRelPath,
    openError,
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
    followRenameUnder,
    reorderTab,
    closeUnder,
    closeOthers,
    closeToTheRight,
    dirtyAmong,
    setAutosaveDelay,
    loadAutosaveDelay,
    setMode,
  };
});
