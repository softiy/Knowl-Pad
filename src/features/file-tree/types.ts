/** 文件树的领域类型与常量（技术方案 §7.3 虚拟滚动；AC-FILE-06/08/09）。 */

/** 扁平化后的**可视行**——虚拟滚动只认这个一维数组，不认树。 */
export interface TreeRow {
  relPath: string;
  name: string;
  isDir: boolean;
  kind: 'note' | 'attachment' | 'other';
  /** 缩进层级：根为 0 */
  depth: number;
  /** 目录是否含可见子项（决定展开箭头） */
  hasChildren: boolean;
  expanded: boolean;
}

/** 右键菜单可触发的动作（FR-FILE-07 的 P0 子集）。 */
export type MenuAction =
  | 'newNote'
  | 'newFolder'
  | 'rename'
  | 'delete'
  | 'reveal'
  | 'copyPath';

/** 每 Vault 的界面状态键（PRD §3.3 的 `vault_state` 键名约定）。 */
export const STATE_KEY_TREE_EXPANDED = 'tree.expanded';
/** 全局偏好键（PRD §3.3 的 `preference` 键名约定）。 */
export const PREF_SHOW_HIDDEN = 'ui.showHiddenFiles';

/** 行高：技术方案 §7.3 规定文件树固定 28px（虚拟滚动要求等高）。 */
export const TREE_ROW_HEIGHT = 28;
/** 可视窗口上下各多渲染的行数（技术方案 §7.3：±10 行缓冲）。 */
export const TREE_OVERSCAN = 10;
