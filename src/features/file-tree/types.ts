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

/** 右键菜单可触发的动作（FR-FILE-07 的 P0 全集；DEBT-19 补 `move` 与 `copyWikilink`）。 */
export type MenuAction =
  | 'newNote'
  | 'newFolder'
  | 'rename'
  | 'move'
  | 'delete'
  | 'reveal'
  | 'copyPath'
  | 'copyWikilink';

/** FR-FILE-02 的排序模式：名称（默认）/ 修改时间 / 类型。 */
export type TreeSortMode = 'name' | 'mtime' | 'type';

/** 排序下拉的选项（标签即界面文案；顺序即下拉顺序）。 */
export const SORT_OPTIONS: readonly { value: TreeSortMode; label: string }[] = [
  { value: 'name', label: '名称' },
  { value: 'mtime', label: '修改时间' },
  { value: 'type', label: '类型' },
];

/** 每 Vault 的界面状态键（PRD §3.3 的 `vault_state` 键名约定）。 */
export const STATE_KEY_TREE_EXPANDED = 'tree.expanded';
/**
 * 每 Vault 的界面状态键：「显示隐藏文件」是否开启。
 *
 * **作用域按 PRD 收敛为 Vault**（§4.11.3 第 18 行「显示隐藏文件 / 关 / **Vault**」+ FR-SET-04）：
 * 存在**全局库的 `vault_state`**（按 `vault_id` 隔离），而不是全局 `preference` ——
 * 后者会让切换 Vault 时串设置（技术方案 §13.2 的 **DEBT-15**，2026-10-17 批次 3 修正）。
 * 键字符串沿用旧名，便于一次性兼容旧版写下的全局值（见 `fileTree.ts::restoreState`）。
 */
export const STATE_KEY_SHOW_HIDDEN = 'ui.showHiddenFiles';

/** 行高：技术方案 §7.3 规定文件树固定 28px（虚拟滚动要求等高）。 */
export const TREE_ROW_HEIGHT = 28;
/** 可视窗口上下各多渲染的行数（技术方案 §7.3：±10 行缓冲）。 */
export const TREE_OVERSCAN = 10;
