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
import { useFileTreeStore } from './fileTree';

import {
  dirOf,
  joinRel,
  makeDialog,
  validateDialogValue,
  type FileOpDialog,
} from './fileOpsDialogState';

// 对话框的形状/构造/校验在 `fileOpsDialogState.ts`（CODE-11 的文件长度纪律）；这里只做编排。
export type { FileOpDialog, FileOpMode } from './fileOpsDialogState';

/**
 * 文件操作编排（FR-FILE-10~13、FR-FILE-30；AC-FILE-03/04/05）。
 *
 * 职责：对话框状态机 + 调用命令 + 操作后刷新文件树 + **让编辑器跟随重命名**（FR-FILE-28）。
 * 命令本身的语义（原子写、覆盖前备份、软删除）在 Rust 侧已保证，这里不重复实现。
 */
export const useFileOpsStore = defineStore('fileOps', () => {
  const dialog = ref<FileOpDialog | null>(null);
  /** 上一次操作的结果提示（成功类，供界面短暂展示） */
  const notice = ref<string | null>(null);
  /** 删除前需要确认（该路径下仍有打开的标签；含未保存的必须先问，R-07） */
  const pendingDelete = ref<{ target: string; open: string[]; dirty: string[] } | null>(null);


  function openNewNote(parentDir: string): void {
    dialog.value = makeDialog('newNote', '', parentDir, '');
  }

  function openNewFolder(parentDir: string): void {
    dialog.value = makeDialog('newFolder', '', parentDir, '');
  }

  /**
   * **由别处发起的创建**（FR-EDITOR-27：点悬空链接 ⇒ 触发创建）：把建议名**预填**进同一个"新建笔记"流程。
   *
   * 只做预填：用户仍可改名，名称校验与冲突三选项（AC-FILE-03/04）照旧 —— **不新增写入路径**。
   */
  function createFromLink(name: string): void {
    dialog.value = makeDialog('newNote', '', '', name.trim());
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

  /** 真正执行删除（软删除 + 关闭该子树下的全部标签 + 刷新树）。 */
  async function performDelete(target: string): Promise<void> {
    const editorStore = useEditorStore();
    const result = await fileDelete(target, true);
    // 关闭该路径及其**子树**下的标签：否则这些标签的自动保存会把已删除的文件写回来
    editorStore.closeUnder(target);
    await refreshTree();
    notice.value = `已移入回收站（${result.trashedCount} 项）`;
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
        notice.value = '仍有未保存内容未能写入，已取消删除';
        return;
      }
    }
    try {
      await performDelete(pending.target);
    } catch (err) {
      notice.value = asKpError(err).message;
    }
    dialog.value = null;
  }

  async function refreshTree(): Promise<void> {
    await useFileTreeStore().refresh();
  }

  /**
   * 提交当前操作。`policy` 仅在目标已存在（conflict=true）时由界面传入（AC-FILE-04 三选项）。
   */
  async function submit(policy: ConflictPolicy = 'cancel'): Promise<void> {
    const current = dialog.value;
    if (!current) return;
    current.error = null;
    if (current.mode !== 'delete' && !(await validateValue())) return;
    current.busy = true;
    try {
      if (current.mode === 'newNote') {
        const relPath = joinRel(current.parentDir, current.value.trim());
        const result = await noteCreate(relPath, '', policy);
        await refreshTree();
        // FR-FILE-10：新建后直接进入编辑态，并**聚焦标题行**（新文件的第一行就是标题的位置）。
        // 走既有的 reveal 通路（FR-LINK-13 同一条），不新造定位机制：它内部会先 openNote，
        // 再把"待定位行"写进 store，由 EditorView 消费一次。
        await useEditorStore().reveal(result.relPath, 1);
        notice.value = `已新建 ${result.relPath}`;
      } else if (current.mode === 'newFolder') {
        const relPath = joinRel(current.parentDir, current.value.trim());
        await folderCreate(relPath);
        await refreshTree();
        notice.value = `已新建文件夹 ${relPath}`;
      } else if (current.mode === 'rename') {
        const to = joinRel(current.parentDir, current.value.trim());
        const result = await fileRename(current.target, to, policy);
        // FR-FILE-28 / AC-FILE-05：编辑器跟随重命名（不丢未保存内容）
        useEditorStore().followRenameUnder(result.from, result.to);
        await refreshTree();
        notice.value = `已重命名为 ${result.to}`;
      } else if (current.mode === 'move') {
        const to = current.value.trim();
        if (to === current.target) {
          current.error = '目标路径与当前相同，无需移动';
          return;
        }
        const result = await fileMove(current.target, to, policy);
        // 移动 = 跨目录的重命名：编辑器跟随（含子树）与刷新逻辑与重命名完全一致
        useEditorStore().followRenameUnder(result.from, result.to);
        await refreshTree();
        notice.value = `已移动到 ${result.to}`;
      } else {
        const editorStore = useEditorStore();
        const prefix = current.target + '/';
        const open = editorStore.buffers
          .map((b) => b.relPath)
          .filter((p) => p === current.target || p.startsWith(prefix));
        const dirty = editorStore.dirtyAmong(open);
        // R-07：删除会关闭该路径及其子树下的标签——有未保存内容时**必须先问**
        if (dirty.length > 0) {
          pendingDelete.value = { target: current.target, open, dirty };
          current.busy = false;
          return;
        }
        await performDelete(current.target);
        return;
      }
      dialog.value = null;
    } catch (err) {
      const kpError = asKpError(err);
      // 目标已存在 → 交给用户选策略（AC-FILE-04：禁止静默覆盖）
      if (kpError.code === 'E_FILE_EXISTS' && current.mode !== 'delete') {
        current.conflict = true;
      }
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
    if (policy === 'cancel') {
      return;
    }
    await submit(policy);
  }

  return {
    dialog,
    notice,
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
