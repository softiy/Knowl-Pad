/**
 * 平台感知的快捷键（NFR-PLAT-07；技术方案 ED-04：**适配器不自行绑定 keydown**，统一走这里）。
 *
 * 约定：`Mod` 在 macOS 表示 `Cmd`、其它平台表示 `Ctrl`；解析与匹配都是纯函数，便于测试。
 */

export interface ShortcutSpec {
  key: string;
  mod: boolean;
  shift: boolean;
  alt: boolean;
}

/** 是否 macOS（决定 Mod 的物理键）。 */
export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Mac|iPhone|iPad|iPod/i.test(navigator.userAgent ?? '');
}

/** 解析形如 `Mod+Shift+S` 的声明；键名大小写不敏感。 */
export function parseShortcut(spec: string): ShortcutSpec {
  const parts = spec.split('+').map((p) => p.trim()).filter(Boolean);
  const result: ShortcutSpec = { key: '', mod: false, shift: false, alt: false };
  for (const part of parts) {
    const token = part.toLowerCase();
    if (token === 'mod' || token === 'ctrl' || token === 'cmd' || token === 'meta') result.mod = true;
    else if (token === 'shift') result.shift = true;
    else if (token === 'alt' || token === 'option') result.alt = true;
    else result.key = token;
  }
  return result;
}

/** 事件是否匹配该声明（要求修饰键精确一致，避免 Ctrl+S 命中 Ctrl+Shift+S）。 */
export function matchesShortcut(event: KeyboardEvent, spec: ShortcutSpec): boolean {
  const modPressed = isMacPlatform() ? event.metaKey : event.ctrlKey;
  return (
    event.key.toLowerCase() === spec.key &&
    modPressed === spec.mod &&
    event.shiftKey === spec.shift &&
    event.altKey === spec.alt
  );
}

export interface ShortcutBinding {
  /** 形如 `Mod+S` */
  spec: string;
  handler: (event: KeyboardEvent) => void;
}

export interface BindShortcutOptions {
  /** 默认 true：避免 WebView 的浏览器默认行为（如 Ctrl+S 保存网页）。 */
  preventDefault?: boolean;
  /**
   * 默认 true：匹配后**停止传播**，避免同一次按键再被子元素上的监听器处理。
   *
   * 这一条是"两个搜索面板"的修复（10.1/10.4）：md-editor-v3 的内核是 CodeMirror 6，
   * 它自己在编辑区 DOM 上监听 keydown（`@codemirror/search` 的 Mod+F/H 会弹出内核搜索面板）。
   * 只在 window 冒泡阶段 preventDefault 拦不住它 —— 改为**捕获阶段**监听并在匹配时 stopPropagation，
   * 事件根本到不了 CodeMirror 的监听器。未匹配的按键不受影响（编辑区照常输入）。
   */
  stopPropagation?: boolean;
}

/**
 * 绑定一组快捷键，返回**注销函数**（ED-01：调用方必须在卸载时调用）。
 *
 * 监听固定在 **capture 阶段**：全局快捷键要抢在内核/组件自己的 keydown 之前处理，
 * 否则 CodeMirror 这类深嵌组件会先把按键吃掉（见 `stopPropagation` 的说明）。
 */
export function bindShortcuts(
  target: Window | HTMLElement,
  bindings: ShortcutBinding[],
  options: BindShortcutOptions = {},
): () => void {
  const parsed = bindings.map((b) => ({ spec: parseShortcut(b.spec), handler: b.handler }));
  const listener = (event: Event): void => {
    const keyEvent = event as KeyboardEvent;
    for (const binding of parsed) {
      if (matchesShortcut(keyEvent, binding.spec)) {
        if (options.preventDefault !== false) keyEvent.preventDefault();
        if (options.stopPropagation !== false) keyEvent.stopPropagation();
        binding.handler(keyEvent);
        return;
      }
    }
  };
  target.addEventListener('keydown', listener, { capture: true });
  return () => target.removeEventListener('keydown', listener, { capture: true });
}
