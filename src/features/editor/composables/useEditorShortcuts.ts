/** 编辑器全局快捷键（Mod+F/H/S；ED-04 统一走 core/shortcut，返回注销函数）。 */
import { bindShortcuts } from '@core/shortcut';

export function bindEditorShortcuts(opts: {
  save: () => void;
  openFind: (mode: 'find' | 'replace') => void;
}): () => void {
  return bindShortcuts(window, [
    { spec: 'Mod+F', handler: () => opts.openFind('find') },
    { spec: 'Mod+H', handler: () => opts.openFind('replace') },
    { spec: 'Mod+S', handler: opts.save },
  ]);
}
