/**
 * 设置项清单（**逐条抄 PRD §4.11.3**，不自行发明；FR-SET-01/02）。
 *
 * `wired` 表示本里程碑是否真的接上行为：theme / autosaveDelayMs / editorFontSize / zoomPercent 已接入；
 * 其余只展示默认值并标注「默认值（未接入）」—— 不假装可调。
 */
export type SettingScope = 'global' | 'vault';

export interface SettingItem {
  id: string;
  category: string;
  label: string;
  defaultText: string;
  scope: SettingScope;
  wired?: 'theme' | 'autosaveDelayMs' | 'editorFontSize' | 'zoomPercent';
}

export const SETTING_CATEGORIES: readonly string[] = [
  '编辑器', '外观', '文件与链接', '搜索', '图谱', '回收站', '更新', '索引', '隐私',
];

export const SETTING_ITEMS: readonly SettingItem[] = [
  { id: 'editor.autosaveDelayMs', category: '编辑器', label: '自动保存防抖时长', defaultText: '1000ms', scope: 'global', wired: 'autosaveDelayMs' },
  { id: 'editor.defaultMode', category: '编辑器', label: '默认编辑模式（编辑/分屏/阅读）', defaultText: '分屏', scope: 'global' },
  { id: 'editor.lineNumbers', category: '编辑器', label: '显示行号', defaultText: '关', scope: 'global' },
  { id: 'editor.spellcheck', category: '编辑器', label: '拼写检查语言', defaultText: '跟随系统', scope: 'global' },
  { id: 'editor.hoverDelayMs', category: '编辑器', label: '悬停预览延迟', defaultText: '300ms', scope: 'global' },
  { id: 'editor.undoDepth', category: '编辑器', label: '撤销栈深度', defaultText: '500', scope: 'global' },

  { id: 'appearance.theme', category: '外观', label: '主题（深/浅/跟随系统）', defaultText: '跟随系统', scope: 'global', wired: 'theme' },
  { id: 'appearance.fontFamily', category: '外观', label: '编辑器字体族', defaultText: '系统等宽', scope: 'global' },
  { id: 'appearance.editorFontSize', category: '外观', label: '编辑器字号', defaultText: '16px', scope: 'global', wired: 'editorFontSize' },
  { id: 'appearance.lineHeight', category: '外观', label: '行高', defaultText: '1.6', scope: 'global' },
  { id: 'appearance.maxWidth', category: '外观', label: '编辑器最大行宽', defaultText: '720px', scope: 'global' },
  { id: 'appearance.zoomPercent', category: '外观', label: '界面缩放', defaultText: '100%', scope: 'global', wired: 'zoomPercent' },

  { id: 'files.newNoteLocation', category: '文件与链接', label: '默认新建位置', defaultText: 'Vault 根目录', scope: 'vault' },
  { id: 'files.attachmentPolicy', category: '文件与链接', label: '附件存储策略', defaultText: '与笔记同目录', scope: 'vault' },
  { id: 'files.linkRewriteMode', category: '文件与链接', label: '链接改写模式（预览确认/自动）', defaultText: '预览确认', scope: 'vault' },
  { id: 'files.linkFormat', category: '文件与链接', label: '新建链接默认格式（最短路径/完整路径）', defaultText: '最短路径', scope: 'vault' },
  { id: 'files.showHidden', category: '文件与链接', label: '显示隐藏文件', defaultText: '关', scope: 'vault' },

  { id: 'search.debounceMs', category: '搜索', label: '搜索防抖', defaultText: '200ms', scope: 'global' },
  { id: 'search.matchMode', category: '搜索', label: '默认匹配模式（AND/OR）', defaultText: 'AND', scope: 'global' },
  { id: 'search.sort', category: '搜索', label: '结果默认排序', defaultText: '相关度', scope: 'global' },
  { id: 'search.snippetLines', category: '搜索', label: '片段上下文行数', defaultText: '2', scope: 'global' },

  { id: 'graph.hops', category: '图谱', label: '局部图谱跳数', defaultText: '2', scope: 'global' },
  { id: 'graph.nodeThreshold', category: '图谱', label: '节点数降级阈值', defaultText: '3000', scope: 'global' },
  { id: 'graph.colorBy', category: '图谱', label: '节点着色依据', defaultText: '文件夹', scope: 'global' },
  { id: 'graph.showOrphans', category: '图谱', label: '显示孤立节点', defaultText: '开', scope: 'global' },
  { id: 'graph.showDangling', category: '图谱', label: '显示悬空链接幽灵节点', defaultText: '关', scope: 'global' },

  { id: 'trash.expireDays', category: '回收站', label: '自动过期天数', defaultText: '30', scope: 'global' },
  { id: 'trash.autoExpire', category: '回收站', label: '启用自动过期清理', defaultText: '开', scope: 'global' },

  { id: 'update.autoCheck', category: '更新', label: '自动检查更新', defaultText: '开', scope: 'global' },
  { id: 'update.frequency', category: '更新', label: '检查频率', defaultText: '每天', scope: 'global' },
  { id: 'update.channel', category: '更新', label: '更新通道', defaultText: 'stable', scope: 'global' },

  { id: 'index.threads', category: '索引', label: '索引线程数', defaultText: 'CPU 核数 - 1', scope: 'global' },
  { id: 'index.maxFileSize', category: '索引', label: '单文件大小上限（超出跳过并警告）', defaultText: '5MB', scope: 'global' },

  { id: 'privacy.remoteImages', category: '隐私', label: '远程图片自动加载', defaultText: '关', scope: 'global' },
];

export function itemsOf(category: string): readonly SettingItem[] {
  return SETTING_ITEMS.filter((item) => item.category === category);
}
