import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import {
  fileDelete,
  fileRename,
  fileValidateName,
  folderCreate,
  noteCreate,
  type ConflictPolicy,
} from '@core/ipc/commands';
import { useEditorStore } from '@features/editor';
import { useFileTreeStore } from './fileTree';

export type FileOpMode = 'newNote' | 'newFolder' | 'rename' | 'delete';

export interface FileOpDialog {
  mode: FileOpMode;
  /** 重命名/删除的目标（源路径）；新建时为空 */
  target: string;
  /** 新建时的父目录（'' 表示 Vault 根） */
  parentDir: string;
  /** 输入值：新建/重命名时为名称 */
  value: string;
  /** 名称校验结果——非法时必须给出**具体原因**（AC-FILE-03 / ERR-02） */
  validation: { valid: boolean; reason?: string | undefined } | null;
  /** 目标已存在，需要用户选择冲突策略（AC-FILE-04） */
  conflict: boolean;
  error: string | null;
  busy: boolean;
}

function dirOf(relPath: string): string {
  const index = relPath.lastIndexOf('/');
  return index < 0 ? '' : relPath.slice(0, index);
}

function joinRel(dir: string, name: string): string {
  return dir ? `${dir}/${name}` : name;
}

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

  function openNewNote(parentDir: string): void {
    dialog.value = {
      mode: 'newNote',
      target: '',
      parentDir,
      value: '',
      validation: null,
      conflict: false,
      error: null,
      busy: false,
    };
  }

  function openNewFolder(parentDir: string): void {
    dialog.value = {
      mode: 'newFolder',
      target: '',
      parentDir,
      value: '',
      validation: null,
      conflict: false,
      error: null,
      busy: false,
    };
  }

  function openRename(relPath: string): void {
    dialog.value = {
      mode: 'rename',
      target: relPath,
      parentDir: dirOf(relPath),
      value: relPath.split('/').pop() ?? relPath,
      validation: null,
      conflict: false,
      error: null,
      busy: false,
    };
  }

  function openDelete(relPath: string): void {
    dialog.value = {
      mode: 'delete',
      target: relPath,
      parentDir: dirOf(relPath),
      value: '',
      validation: null,
      conflict: false,
      error: null,
      busy: false,
    };
  }

  function close(): void {
    dialog.value = null;
  }

  /** 校验输入的名称（AC-FILE-03）：非法时把**具体原因**放进 dialog.validation。 */
  async function validateValue(): Promise<boolean> {
    const current = dialog.value;
    if (!current || current.mode === 'delete') return true;
    if (!current.value.trim()) {
      current.validation = { valid: false, reason: '名称不能为空' };
      return false;
    }
    const result = await fileValidateName(current.value);
    current.validation = result;
    return result.valid;
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
        // FR-FILE-10：新建后直接进入编辑态
        await useEditorStore().openNote(result.relPath);
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
        useEditorStore().followRename(result.from, result.to);
        await refreshTree();
        notice.value = `已重命名为 ${result.to}`;
      } else {
        const result = await fileDelete(current.target, true);
        // 文件已移入回收站：关闭对应编辑器标签（避免标签指向不存在的文件）
        useEditorStore().closeNote(current.target);
        await refreshTree();
        notice.value = `已移入回收站（${result.trashedCount} 项）`;
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
    openNewFolder,
    openRename,
    openDelete,
    validateValue,
    submit,
    resolveConflict,
    close,
  };
});
