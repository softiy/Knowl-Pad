/**
 * 文件操作对话框的**状态形状与校验**（从 `fileOps` store 拆出；CODE-11 的文件长度纪律）。
 *
 * 拆出的理由：store 本身已接近预警线，而"对话框长什么样 / 名称怎么校验"是**与编排无关**的一层 ——
 * 编排（调命令、刷新树、让编辑器跟随）留在 `fileOps.ts`，这里只放形状、构造与校验。
 */
import { fileValidateName } from '@core/ipc/commands';

/**
 * 对话框模式。`move` 是 DEBT-19 补的（`file_move` 此前**没有 UI 入口**）：语义与重命名同一条命令
 * （跨目录即移动），区别只在输入的是**目标路径**而不是单个名称。
 */
export type FileOpMode = 'newNote' | 'newFolder' | 'rename' | 'move' | 'delete';

export interface FileOpDialog {
  mode: FileOpMode;
  /** 重命名/移动/删除的目标（源路径）；新建时为空 */
  target: string;
  /** 新建时的父目录（'' 表示 Vault 根） */
  parentDir: string;
  /** 输入值：新建/重命名时为名称；移动时为**目标路径** */
  value: string;
  /** 名称校验结果——非法时必须给出**具体原因**（AC-FILE-03 / ERR-02） */
  validation: { valid: boolean; reason?: string | undefined } | null;
  /** 目标已存在，需要用户选择冲突策略（AC-FILE-04） */
  conflict: boolean;
  /**
   * **FR-FILE-32 / AC-FILE-11**：删除前统计到的**引用处数**。
   * `null` = 不适用（非笔记/还没取到）；`0` = 确实没有引用（界面不得提示，避免噪声）。
   */
  referenceCount: number | null;
  /** 引用统计失败的可读原因（R-15：如实提示"无法统计"，不用 0 冒充）。 */
  referenceError: string | null;
  error: string | null;
  busy: boolean;
}

/** 路径的目录部分（'' 表示根）。 */
export function dirOf(relPath: string): string {
  const index = relPath.lastIndexOf('/');
  return index < 0 ? '' : relPath.slice(0, index);
}

/** 拼回相对路径。 */
export function joinRel(dir: string, name: string): string {
  return dir ? `${dir}/${name}` : name;
}

/** 建立一个对话框的初始状态（各模式共用同一形状，避免四处抄字段）。 */
export function makeDialog(
  mode: FileOpMode,
  target: string,
  parentDir: string,
  value: string,
): FileOpDialog {
  return {
    mode,
    target,
    parentDir,
    value,
    validation: null,
    conflict: false,
    referenceCount: null,
    referenceError: null,
    error: null,
    busy: false,
  };
}

/**
 * 校验输入（AC-FILE-03）：非法时把**具体原因**写进 `dialog.validation` 并返回 false。
 *
 * 删除模式不需要校验；移动模式输入的是路径，**只对最后一段**做名称校验（目录段的合法性由 Rust 侧的
 * 七步校验负责，SEC-02）。
 */
export async function validateDialogValue(current: FileOpDialog): Promise<boolean> {
  if (current.mode === 'delete') return true;
  const raw = current.value.trim();
  if (!raw) {
    current.validation = {
      valid: false,
      reason: current.mode === 'move' ? '目标路径不能为空' : '名称不能为空',
    };
    return false;
  }
  const name = current.mode === 'move' ? (raw.split('/').pop() ?? '') : raw;
  const result = await fileValidateName(name);
  current.validation = result;
  return result.valid;
}