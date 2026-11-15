import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import {
  fileDelete,
  fileMove,
  fileRename,
  folderCreate,
  noteCreate,
  type ConflictPolicy,
} from '@core/ipc/commands';
import { useEditorStore } from '@features/editor';
import { countReferences } from '../deleteReferences';
import { ensureNoteExtension, isSameRelPath, normalizeRelPath } from '../logic';
import { useFileTreeStore } from './fileTree';
import { createNoticeController } from './fileNotice';
import { caseInsensitiveFs } from './platformFs';

import {
  DEFAULT_NOTE_NAME,
  dirOf,
  joinRel,
  makeDialog,
  validateDialogValue,
  type FileOpDialog,
} from './fileOpsDialogState';

// 对话框的形状/构造/校验在 `fileOpsDialogState.ts`、提示的自动清除在 `fileNotice.ts`
// （CODE-11 的文件长度纪律）；这里只做编排。
export type { FileOpDialog, FileOpMode } from './fileOpsDialogState';

/**
 * 文件操作编排（FR-FILE-10~13、FR-FILE-30；AC-FILE-03/04/05）。
 *
 * 职责：对话框状态机 + 调用命令 + 操作后**本地同步文件树** + **让编辑器跟随重命名**（FR-FILE-28）。
 * 命令本身的语义（原子写、覆盖前备份、软删除）在 Rust 侧已保证，这里不重复实现。
 *
 * 为什么操作后不是"重新拉取树"而是本地改树：文件树的数据源是**索引库**，而索引由文件监听
 * 异步更新（防抖 200ms + 事务）—— 命令刚返回时重新拉取拿到的仍是旧索引，于是「新建的笔记不在树里」
 * 「删掉的笔记还在树里」。本地先按命令结果更新（即时反馈），refresh / 重开 Vault 时再以索引为准。
 */
export const useFileOpsStore = defineStore('fileOps', () => {
  const dialog = ref<FileOpDialog | null>(null);
  /** 操作结果提示：设置后自动清除（规则见 fileNotice.ts），切库时由 clearNotice() 清掉 */
  const { notice, noticeKind, setNotice, clearNotice } = createNoticeController();
  /** 删除前需要确认（该路径下仍有打开的标签；含未保存的必须先问，R-07） */
  const pendingDelete = ref<{ target: string; open: string[]; dirty: string[] } | null>(null);

  function openNewNote(parentDir: string): void {
    // M5 冒烟 5.6：默认名**带 .md**（否则会建出无扩展名文件，编辑器/索引都不当它是笔记）
    dialog.value = makeDialog('newNote', '', parentDir, DEFAULT_NOTE_NAME);
  }

  function openNewFolder(parentDir: string): void {
    dialog.value = makeDialog('newFolder', '', parentDir, '');
  }

  /**
   * **由别处发起的创建**（FR-EDITOR-27：点悬空链接 ⇒ 触发创建）：把建议名**预填**进同一个"新建笔记"流程。
   *
   * 只做预填：用户仍可改名，名称校验与冲突三选项（AC-FILE-03/04）照旧 —— **不新增写入路径**。
   * 链接目标常常不带扩展名，这里补成 .md（否则就是 M5 冒烟 5.6 的"新建笔记没有扩展名"）。
   */
  function createFromLink(name: string): void {
    dialog.value = makeDialog('newNote', '', '', ensureNoteExtension(name));
  }

  function openRename(relPath: string): void {
    dialog.value = makeDialog('rename', relPath, dirOf(relPath), relPath.split('/').pop() ?? relPath);
  }

  /**
   * FR-FILE-07 / DEBT-19：**移动到…**（`file_move` 的 UI 入口）。
   *
   * 预填完整目标路径（= 当前位置），用户改目录部分即可；提交时走与重命名同一条命令，
   * 因此冲突三选项（AC-FILE-04）与"编辑器跟随"（FR-FILE-28）都照旧生效。
   */
  function openMove(relPath: string): void {
    dialog.value = makeDialog('move', relPath, dirOf(relPath), relPath);
  }

  /**
   * 打开删除确认（FR-FILE-30）并**异步**统计引用处数（**FR-FILE-32 / DEBT-29**）。
   *
   * 顺序刻意如此：对话框先出现（用户马上看到"确定要删吗"），计数随后填进同一个对话框 ——
   * 统计失败也**不阻断**删除，只是在界面上如实说明"无法统计"（R-15）。
   */
  function openDelete(relPath: string): void {
    dialog.value = makeDialog('delete', relPath, dirOf(relPath), '');
    void applyReferences(relPath);
  }

  /** 把引用数写回**仍然有效**的删除对话框（用户可能已经关掉或换成了别的目标）。 */
  async function applyReferences(relPath: string): Promise<void> {
    const summary = await countReferences(relPath);
    const current = dialog.value;
    if (current === null || current.mode !== 'delete' || current.target !== relPath) return;
    current.referenceCount = summary.count;
    current.referenceError = summary.error;
  }

  function close(): void {
    dialog.value = null;
  }

  /** 校验输入（AC-FILE-03）：具体规则在 `validateDialogValue`（名称/路径两种口径）。 */
  async function validateValue(): Promise<boolean> {
    const current = dialog.value;
    if (!current) return true;
    return validateDialogValue(current);
  }

  /**
   * 提交前归一输入（M5 冒烟 5.4/5.6）：
   * - 新建笔记补 `.md`；移动目标统一 `/` 分隔并去首尾空白（用户可能手输反斜杠）；
   * - 名称类去首尾空白。
   */
  function normalizeInput(current: FileOpDialog): void {
    if (current.mode === 'newNote') current.value = ensureNoteExtension(current.value);
    else if (current.mode === 'move') current.value = normalizeRelPath(current.value);
    else if (current.mode !== 'delete') current.value = current.value.trim();
  }

  /**
   * 目标是否与当前位置**相同**（M5 冒烟 5.4：把移动目标改回原路径不应发命令）。
   *
   * 大小写口径按**平台事实**（`vault_current.caseInsensitiveFs`，来自 Rust platform 层）：
   * Windows/macOS 默认不区分大小写，`归档/A.md` 与 `归档/a.md` 是同一个文件，
   * 放行只会让 file_move 静默地什么都不做（用户看到"已移动"却没有变化）。R-09：不硬编码平台。
   */
  async function sameTarget(to: string, from: string): Promise<boolean> {
    if (normalizeRelPath(to) === normalizeRelPath(from)) return true;
    return isSameRelPath(to, from, await caseInsensitiveFs());
  }

  /** 真正执行删除（软删除 + 关闭该子树下的全部标签 + 本地移除树节点）。 */
  async function performDelete(target: string): Promise<void> {
    const editorStore = useEditorStore();
    const result = await fileDelete(target, true);
    // 关闭该路径及其**子树**下的标签：否则这些标签的自动保存会把已删除的文件写回来
    editorStore.closeUnder(target);
    useFileTreeStore().pathRemoved(target);
    setNotice(`已移入回收站（${result.trashedCount} 项）`);
  }

  /** 用户在「未保存」确认条上的选择：save=保存后删除 / discard=直接删除 / cancel=取消。 */
  async function resolveDelete(choice: 'save' | 'discard' | 'cancel'): Promise<void> {
    const pending = pendingDelete.value;
    pendingDelete.value = null;
    if (!pending || choice === 'cancel') return;
    const editorStore = useEditorStore();
    if (choice === 'save') {
      await Promise.all(pending.dirty.map((p) => editorStore.save(p)));
      // 保存仍失败（写冲突/权限）时不得继续删除，避免静默丢内容
      if (editorStore.dirtyAmong(pending.dirty).length > 0) {
        setNotice('仍有未保存内容未能写入，已取消删除', 'error');
        return;
      }
    }
    try {
      await performDelete(pending.target);
      dialog.value = null;
    } catch (err) {
      // 失败时**保持对话框**并给出红字原因（可重试 / 取消），不静默
      const message = asKpError(err).message;
      if (dialog.value) dialog.value.error = message;
      setNotice(message, 'error');
    }
  }

  /**
   * 执行当前对话框对应的命令（成功时关闭对话框；错误统一由 `submit` 处理成对话框里的红字）。
   * 从 submit 拆出的理由：CODE-11 的函数级长度上限，语义未变。
   */
  async function runOperation(current: FileOpDialog, policy: ConflictPolicy): Promise<void> {
    const tree = useFileTreeStore();
    if (current.mode === 'newNote') {
      const relPath = joinRel(current.parentDir, current.value);
      const result = await noteCreate(relPath, '', policy);
      // 本地插入 + 展开父目录（索引是异步的，见模块头注释），随后定位到第 1 行
      await tree.nodeAdded(result.relPath, { mtimeMs: result.newMtime });
      await useEditorStore().reveal(result.relPath, 1);
      setNotice(`已新建 ${result.relPath}`);
      dialog.value = null;
      return;
    }
    if (current.mode === 'newFolder') {
      const relPath = joinRel(current.parentDir, current.value);
      await folderCreate(relPath);
      await tree.nodeAdded(relPath, { isDir: true });
      setNotice(`已新建文件夹 ${relPath}`);
      dialog.value = null;
      return;
    }
    if (current.mode === 'rename') {
      const to = joinRel(current.parentDir, current.value);
      const result = await fileRename(current.target, to, policy);
      // FR-FILE-28 / AC-FILE-05：编辑器跟随重命名（不丢未保存内容）
      useEditorStore().followRenameUnder(result.from, result.to);
      tree.pathMoved(result.from, result.to);
      setNotice(`已重命名为 ${result.to}`);
      dialog.value = null;
      return;
    }
    if (current.mode === 'move') {
      // M5 冒烟 5.4：目标 == 当前位置 ⇒ **不发命令**，红字说明（大小写按平台口径，见 sameTarget）
      if (await sameTarget(current.value, current.target)) {
        current.error = '与当前相同，无需移动';
        return;
      }
      const result = await fileMove(current.target, current.value, policy);
      // 移动 = 跨目录的重命名：编辑器跟随（含子树）与本地树同步与重命名完全一致
      useEditorStore().followRenameUnder(result.from, result.to);
      tree.pathMoved(result.from, result.to);
      setNotice(`已移动到 ${result.to}`);
      dialog.value = null;
      return;
    }
    // 删除（FR-FILE-30）：先确认该路径下未保存的标签（R-07），再真正删除
    const editorStore = useEditorStore();
    const prefix = current.target + '/';
    const open = editorStore.buffers
      .map((buffer) => buffer.relPath)
      .filter((relPath) => relPath === current.target || relPath.startsWith(prefix));
    const dirty = editorStore.dirtyAmong(open);
    if (dirty.length > 0) {
      pendingDelete.value = { target: current.target, open, dirty };
      return;
    }
    await performDelete(current.target);
    dialog.value = null;
  }

  /**
   * 提交当前操作。`policy` 仅在目标已存在（conflict=true）时由界面传入（AC-FILE-04 三选项）。
   */
  async function submit(policy: ConflictPolicy = 'cancel'): Promise<void> {
    const current = dialog.value;
    if (!current) return;
    current.error = null;
    normalizeInput(current);
    if (current.mode !== 'delete' && !(await validateValue())) return;
    current.busy = true;
    try {
      await runOperation(current, policy);
    } catch (err) {
      const kpError = asKpError(err);
      // 目标已存在 → 交给用户选策略（AC-FILE-04：禁止静默覆盖）
      if (kpError.code === 'E_FILE_EXISTS' && current.mode !== 'delete') current.conflict = true;
      current.error = kpError.message;
    } finally {
      current.busy = false;
    }
  }

  /** 用户选择冲突策略后重试（覆盖 / 重命名新建 / 取消）。 */
  async function resolveConflict(policy: ConflictPolicy): Promise<void> {
    const current = dialog.value;
    if (!current) return;
    current.conflict = false;
    if (policy === 'cancel') return;
    await submit(policy);
  }

  return {
    dialog,
    notice,
    noticeKind,
    setNotice,
    clearNotice,
    openNewNote,
    createFromLink,
    openNewFolder,
    openRename,
    openMove,
    openDelete,
    validateValue,
    submit,
    resolveConflict,
    pendingDelete,
    resolveDelete,
    close,
  };
});
