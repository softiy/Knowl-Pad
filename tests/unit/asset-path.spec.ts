import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resolveAssetSrc } from '@core/markdown/assetPath';
import { assetUrlFor } from '@core/ipc/assetCommands';

/**
 * E1 的**纯路径解析**回归（FR-EDITOR-43）：阅读态本地图片此前整条链没接线，
 * 修好后必须保证"相对当前笔记的 src ⇒ Vault 内绝对路径"这一步既不越界也不误伤
 * （markdown-it 会把空格/中文百分号编码，Windows 还有反斜杠）。
 *
 * 这里用普通字符串常量而不是 String.raw：断言里的 `\\` 就是文件字节，读起来更直白。
 */
const WIN_ROOT = 'C:\\Users\\me\\vault';

/** Tauri 运行时替身入口（setup.ts 只挂了 invoke；convertFileSrc 是 asset 协议专用）。 */
type AssetRuntime = { convertFileSrc?: (filePath: string, protocol?: string) => string };
const runtime = (): AssetRuntime | undefined =>
  (globalThis as { __TAURI_INTERNALS__?: AssetRuntime }).__TAURI_INTERNALS__;

describe('resolveAssetSrc：相对当前笔记解析本地图片（E1）', () => {
  it('相对当前笔记目录解析（含 ./ 与 ../）', () => {
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', 'img/x.png')).toBe(
      'C:\\Users\\me\\vault\\notes\\img\\x.png',
    );
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', './img/x.png')).toBe(
      'C:\\Users\\me\\vault\\notes\\img\\x.png',
    );
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', '../assets/x.png')).toBe(
      'C:\\Users\\me\\vault\\assets\\x.png',
    );
    // 根目录笔记：src 直接相对 Vault 根
    expect(resolveAssetSrc(WIN_ROOT, 'a.md', 'attachments/x.png')).toBe(
      'C:\\Users\\me\\vault\\attachments\\x.png',
    );
  });

  it('百分号编码与空格解码（markdown-it 的正常输出）；反斜杠归一', () => {
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', 'img/%E4%B8%AD%E6%96%87%20a.png')).toBe(
      'C:\\Users\\me\\vault\\notes\\img\\中文 a.png',
    );
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', 'img\\a.png')).toBe(
      'C:\\Users\\me\\vault\\notes\\img\\a.png',
    );
    // 非法百分号序列（裸 %）按字面量处理，不让整篇渲染抛异常
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', 'img/100%.png')).toBe(
      'C:\\Users\\me\\vault\\notes\\img\\100%.png',
    );
  });

  it('非 Windows 根目录用 / 分隔（跨平台同一语义）', () => {
    expect(resolveAssetSrc('/home/me/vault', 'notes/sub/a.md', '../assets/x.png')).toBe(
      '/home/me/vault/notes/assets/x.png',
    );
  });

  it('远程 / 协议 URL 一律 null（SEC-08：应用内不加载远程资源）', () => {
    for (const src of [
      'https://evil.example/a.png',
      'http://evil.example/a.png',
      '//cdn.example/a.png',
      'data:image/svg+xml,<svg onload=alert(1)>',
      'file:///C:/Windows/x.png',
      'asset://localhost/C:/x.png',
      'javascript:alert(1)',
      // 编码后的协议 URL 同样不能当本地路径
      'https%3A%2F%2Fevil.example%2Fa.png',
    ]) {
      expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', src)).toBeNull();
    }
  });

  it('绝对路径 / 越界 .. 一律 null（不悄悄夹回 Vault 根）', () => {
    for (const src of [
      '/etc/passwd',
      'C:/Windows/win.ini',
      String.raw`\\server\share\x.png`,
      '../../outside.png',
      '../../../../etc/passwd',
      '%2e%2e%2f%2e%2e%2foutside.png',
    ]) {
      expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', src)).toBeNull();
    }
  });

  it('空 src / 空 vault root / 控制字符 ⇒ null', () => {
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', '   ')).toBeNull();
    expect(resolveAssetSrc('  ', 'notes/a.md', 'a.png')).toBeNull();
    expect(resolveAssetSrc(WIN_ROOT, 'notes/a.md', 'a%00b.png')).toBeNull();
  });

  it('笔记 relPath 的反斜杠也能解析（Windows store 口径）', () => {
    expect(resolveAssetSrc(WIN_ROOT, 'notes\\sub\\a.md', 'x.png')).toBe(
      'C:\\Users\\me\\vault\\notes\\sub\\x.png',
    );
  });
});

describe('assetUrlFor：绝对路径 ⇒ asset URL（E1 / 转换交给 Tauri）', () => {
  let savedConvert: AssetRuntime['convertFileSrc'];

  beforeEach(() => {
    savedConvert = runtime()?.convertFileSrc;
  });

  afterEach(() => {
    const target = runtime();
    if (!target) return;
    if (savedConvert === undefined) delete target.convertFileSrc;
    else target.convertFileSrc = savedConvert;
  });

  it('解析成功时调用 convertFileSrc（Windows 形态由 Tauri 决定，不手拼）', () => {
    const convert = vi.fn((filePath: string) => `http://asset.localhost/${encodeURIComponent(filePath)}`);
    const target = runtime();
    expect(target).toBeDefined();
    if (!target) return;
    target.convertFileSrc = convert;

    const url = assetUrlFor(WIN_ROOT, 'notes/a.md', '../assets/%E5%9B%BE%201.png');
    const expectedAbs = 'C:\\Users\\me\\vault\\assets\\图 1.png';
    // 第二个参数是协议名（convertFileSrc 默认 'asset'，由 Tauri 决定 URL 形态）
    expect(convert).toHaveBeenCalledWith(expectedAbs, 'asset');
    expect(url).toBe(`http://asset.localhost/${encodeURIComponent(expectedAbs)}`);
  });

  it('解析失败（远程/越界）⇒ null 且**不调用** convertFileSrc', () => {
    const convert = vi.fn(() => 'http://asset.localhost/x');
    const target = runtime();
    if (target) target.convertFileSrc = convert;
    expect(assetUrlFor(WIN_ROOT, 'notes/a.md', 'https://evil.example/a.png')).toBeNull();
    expect(assetUrlFor(WIN_ROOT, 'notes/a.md', '../../outside.png')).toBeNull();
    expect(convert).not.toHaveBeenCalled();
  });

  it('没有 Tauri 运行时（浏览器预览 / 单测）⇒ null，不让渲染抛异常', () => {
    const target = runtime();
    if (target) delete target.convertFileSrc;
    expect(assetUrlFor(WIN_ROOT, 'notes/a.md', 'img/a.png')).toBeNull();
  });
});
