import { describe, expect, it, vi } from 'vitest';
import { bindShortcuts, matchesShortcut, parseShortcut } from '@core/shortcut';

function keyEvent(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent('keydown', { cancelable: true, ...init });
}

describe('parseShortcut / matchesShortcut（NFR-PLAT-07）', () => {
  it('解析 Mod/Sift/Alt 与键名（大小写不敏感）', () => {
    expect(parseShortcut('Mod+S')).toEqual({ key: 's', mod: true, shift: false, alt: false });
    expect(parseShortcut('mod+shift+z')).toEqual({ key: 'z', mod: true, shift: true, alt: false });
    expect(parseShortcut('Alt+Enter')).toEqual({ key: 'enter', mod: false, shift: false, alt: true });
  });

  it('非 macOS 上 Mod 映射到 Ctrl', () => {
    const spec = parseShortcut('Mod+S');
    expect(matchesShortcut(keyEvent({ key: 's', ctrlKey: true }), spec)).toBe(true);
    expect(matchesShortcut(keyEvent({ key: 's' }), spec)).toBe(false);
  });

  it('修饰键必须精确一致（Ctrl+S 不得命中 Ctrl+Shift+S）', () => {
    const spec = parseShortcut('Mod+S');
    expect(matchesShortcut(keyEvent({ key: 's', ctrlKey: true, shiftKey: true }), spec)).toBe(false);
  });

  it('macOS 上 Mod 映射到 Cmd', () => {
    const original = navigator.userAgent;
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true });
    try {
      const spec = parseShortcut('Mod+S');
      expect(matchesShortcut(keyEvent({ key: 's', metaKey: true }), spec)).toBe(true);
      expect(matchesShortcut(keyEvent({ key: 's', ctrlKey: true }), spec)).toBe(false);
    } finally {
      Object.defineProperty(navigator, 'userAgent', { value: original, configurable: true });
    }
  });
});

describe('bindShortcuts（ED-04）', () => {
  it('触发处理器并默认 preventDefault', () => {
    const handler = vi.fn();
    const dispose = bindShortcuts(window, [{ spec: 'Mod+S', handler }]);
    const event = keyEvent({ key: 's', ctrlKey: true });
    window.dispatchEvent(event);
    expect(handler).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
    dispose();
  });

  it('注销后不再触发（ED-01 的资源释放）', () => {
    const handler = vi.fn();
    const dispose = bindShortcuts(window, [{ spec: 'Mod+S', handler }]);
    dispose();
    window.dispatchEvent(keyEvent({ key: 's', ctrlKey: true }));
    expect(handler).not.toHaveBeenCalled();
  });

  it('可选关闭 preventDefault', () => {
    const handler = vi.fn();
    const dispose = bindShortcuts(window, [{ spec: 'Mod+S', handler }], { preventDefault: false });
    const event = keyEvent({ key: 's', ctrlKey: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    dispose();
  });
});
