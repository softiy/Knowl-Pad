/**
 * 全局快捷键：`Mod+P` 开面板、`Esc` 关（FR-PALETTE-01）。
 *
 * 统一走 `core/shortcut` 的 `bindShortcuts`（ED-04：组件不自行绑 keydown），返回注销函数（ED-01）。
 */
import { bindShortcuts } from '@core/shortcut';

export interface PaletteShortcutHandlers {
  open(): void;
  close(): void;
}

export function registerPaletteShortcut(win: Window, handlers: PaletteShortcutHandlers): () => void {
  return bindShortcuts(win, [
    {
      spec: 'Mod+P',
      handler: () => {
        handlers.open();
      },
    },
    {
      spec: 'Escape',
      handler: () => {
        handlers.close();
      },
    },
  ]);
}
