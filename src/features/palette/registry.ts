/**
 * 命令面板的**命令注册表**（FR-PALETTE-02/03/05）。
 *
 * 设计要点：
 * - 纯数据 + 纯判定函数：UI 只负责渲染与键盘；**执行**也由调用方注入（`PaletteActions`），
 *   这样命令面板不直接依赖任何 feature 的 store（FE-02 的依赖方向），也便于单测替身；
 * - 只登记**真实存在**的内置命令 —— 图谱 / 设置 / 主题在本里程碑尚未实现（prd FR-PALETTE-03 的示例里提到，
 *   但代码里没有对应能力），本轮**不编造**占位命令；
 * - 拼音首字母由**内置静态表**提供（keywords 里的拼音缩写），不引入第三方拼音库（R-16 / FR-PALETTE-02）。
 */
export interface PaletteContext {
  hasActiveNote: boolean;
  hasVault: boolean;
  mode: 'edit' | 'read';
  includeHidden: boolean;
}

/** 命令的执行体由布局层注入（各 feature 的 store 只在那一层接线）。 */
export interface PaletteActions {
  newNote(): void | Promise<void>;
  quickOpen(): void | Promise<void>;
  search(): void | Promise<void>;
  tags(): void | Promise<void>;
  saveNote(): void | Promise<void>;
  saveAll(): void | Promise<void>;
  closeNote(): void | Promise<void>;
  toggleMode(): void | Promise<void>;
  switchVault(): void | Promise<void>;
  refreshTree(): void | Promise<void>;
  toggleHidden(): void | Promise<void>;
  openSettings(): void | Promise<void>;
  toggleTheme(): void | Promise<void>;
  toggleSidebar(): void | Promise<void>;
  toggleRightPanel(): void | Promise<void>;
}

export type PaletteGroup = 'command';

export interface PaletteCommand {
  id: string;
  name: string;
  /** 单字图标（不引入图标库；PRD 只要求"显示图标"）。 */
  icon: string;
  group: PaletteGroup;
  /** 模糊匹配的附加词：拼音缩写 + 英文别名（仅命令名参与拼音匹配）。 */
  keywords: string[];
  /** 展示用快捷键（若有）。 */
  keys?: string;
  /** 可用性：缺省恒可用。 */
  needs?: (ctx: PaletteContext) => boolean;
  /** 不可用原因（置灰时显示）。 */
  reason?: (ctx: PaletteContext) => string;
  run: (actions: PaletteActions) => void | Promise<void>;
}

export const EDITOR_REASON = '当前无打开的笔记';
export const VAULT_REASON = '尚未打开 Vault';

const editorOnly = (ctx: PaletteContext): boolean => ctx.hasActiveNote;
const editorReason = (): string => EDITOR_REASON;
const vaultOnly = (ctx: PaletteContext): boolean => ctx.hasVault;
const vaultReason = (): string => VAULT_REASON;

/** 内置命令（只含真实能力；顺序即空查询时的默认顺序）。 */
export const COMMANDS: readonly PaletteCommand[] = [
  {
    id: 'palette.new-note',
    name: '新建笔记',
    icon: '＋',
    group: 'command',
    keywords: ['xinjian', 'xj', 'new', 'note'],
    needs: vaultOnly,
    reason: vaultReason,
    run: (a) => a.newNote(),
  },
  {
    id: 'palette.quick-open',
    name: '快速打开',
    icon: '⌕',
    group: 'command',
    keywords: ['kuaisudakai', 'ksdk', 'open', 'quick'],
    keys: 'Mod+O',
    run: (a) => a.quickOpen(),
  },
  {
    id: 'palette.search',
    name: '全文搜索',
    icon: '🔍',
    group: 'command',
    keywords: ['quanwensousuo', 'qws', 'search'],
    keys: 'Mod+Shift+F',
    run: (a) => a.search(),
  },
  {
    id: 'palette.tags',
    name: '打开标签面板',
    icon: '＃',
    group: 'command',
    keywords: ['biaoqian', 'bq', 'tags'],
    keys: 'Mod+Shift+T',
    run: (a) => a.tags(),
  },
  {
    id: 'palette.save-note',
    name: '保存当前笔记',
    icon: '💾',
    group: 'command',
    keywords: ['baocun', 'bc', 'save'],
    needs: editorOnly,
    reason: editorReason,
    run: (a) => a.saveNote(),
  },
  {
    id: 'palette.save-all',
    name: '保存全部笔记',
    icon: '🗂',
    group: 'command',
    keywords: ['baocunquanbu', 'bcqb', 'saveall'],
    run: (a) => a.saveAll(),
  },
  {
    id: 'palette.close-note',
    name: '关闭当前笔记',
    icon: '✕',
    group: 'command',
    keywords: ['guanbi', 'gb', 'close'],
    needs: editorOnly,
    reason: editorReason,
    run: (a) => a.closeNote(),
  },
  {
    id: 'palette.toggle-mode',
    name: '切换阅读 / 编辑模式',
    icon: '⇄',
    group: 'command',
    keywords: ['qiehuan', 'qh', 'mode', 'read', 'edit'],
    needs: editorOnly,
    reason: editorReason,
    run: (a) => a.toggleMode(),
  },
  {
    id: 'palette.switch-vault',
    name: '切换知识库',
    icon: '⇆',
    group: 'command',
    keywords: ['qiehuanvault', 'qhv', 'switch', 'vault'],
    run: (a) => a.switchVault(),
  },
  {
    id: 'palette.refresh-tree',
    name: '刷新文件树',
    icon: '↻',
    group: 'command',
    keywords: ['shuaxin', 'sx', 'refresh'],
    needs: vaultOnly,
    reason: vaultReason,
    run: (a) => a.refreshTree(),
  },
  {
    id: 'palette.toggle-sidebar',
    name: '切换左侧栏（折叠/展开）',
    icon: '▤',
    group: 'command',
    keywords: ['cebianlan', 'cbl', 'sidebar', 'collapse'],
    run: (a) => a.toggleSidebar(),
  },
  {
    id: 'palette.toggle-right-panel',
    name: '切换右侧面板',
    icon: '▥',
    group: 'command',
    keywords: ['youce', 'ycmb', 'panel', 'right'],
    run: (a) => a.toggleRightPanel(),
  },
  {
    id: 'palette.settings',
    name: '打开设置',
    icon: '⚙',
    group: 'command',
    keywords: ['shezhi', 'sz', 'settings', 'preferences'],
    run: (a) => a.openSettings(),
  },
  {
    id: 'palette.toggle-theme',
    name: '切换主题（浅色/深色）',
    icon: '◐',
    group: 'command',
    keywords: ['zhuti', 'zt', 'theme', 'dark', 'light'],
    run: (a) => a.toggleTheme(),
  },
  {
    id: 'palette.toggle-hidden',
    name: '显示 / 隐藏隐藏文件',
    icon: '◌',
    group: 'command',
    keywords: ['yincang', 'yc', 'hidden'],
    needs: vaultOnly,
    reason: vaultReason,
    run: (a) => a.toggleHidden(),
  },
];

export function isEnabled(command: PaletteCommand, ctx: PaletteContext): boolean {
  return command.needs === undefined ? true : command.needs(ctx);
}

export function disabledReason(command: PaletteCommand, ctx: PaletteContext): string {
  return command.reason === undefined ? '' : command.reason(ctx);
}
