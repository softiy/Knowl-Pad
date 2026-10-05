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

/**
 * 绑定一组快捷键，返回**注销函数**（ED-01：调用方必须在卸载时调用）。
 * 默认 `preventDefault`，避免 WebView 的浏览器默认行为（如 Ctrl+S 保存网页）。
 */
export function bindShortcuts(
  target: Window | HTMLElement,
  bindings: ShortcutBinding[],
  options: { preventDefault?: boolean } = {},
): () => void {
  const parsed = bindings.map((b) => ({ spec: parseShortcut(b.spec), handler: b.handler }));
  const listener = (event: Event): void => {
    const keyEvent = event as KeyboardEvent;
    for (const binding of parsed) {
      if (matchesShortcut(keyEvent, binding.spec)) {
        if (options.preventDefault !== false) keyEvent.preventDefault();
        binding.handler(keyEvent);
        return;
      }
    }
  };
  target.addEventListener('keydown', listener);
  return () => target.removeEventListener('keydown', listener);
}
