#!/usr/bin/env node
/**
 * 生成桌面端应用图标（Windows .ico / macOS .icns / Linux .png）。
 *
 * 源图：`assets/brand/knowl-pad-icon-1024.png`
 *   由同目录的 `knowl-pad-icon.svg` 以 1024×1024 透明背景导出，是唯一设计源。
 *   ⚠️ 修改 SVG 后必须重新导出 1024 PNG，否则本脚本仍会消费旧图。
 * 用法：`node scripts/gen-icons.mjs`
 *
 * 为什么调用 `tauri icon` 而不是手写 ICO/ICNS：
 *   多尺寸 ICO、ICNS 与各平台 PNG 的编码细节由 Tauri 维护，手写结构容易在
 *   不同 Windows/macOS 版本上出现尺寸或色深兼容问题。这里只负责把 Tauri
 *   生成的**桌面**产物放进 `src-tauri/icons/`（本项目不支持移动端）。
 */
import { execFileSync } from 'node:child_process';
import { cp, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const source = join(root, 'assets', 'brand', 'knowl-pad-icon-1024.png');
const target = join(root, 'src-tauri', 'icons');
const tauriCli = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

/** 桌面端需要的产物；不包含 tauri icon 额外生成的 android/ios 资源。 */
const desktopFiles = [
  '32x32.png',
  '64x64.png',
  '128x128.png',
  '128x128@2x.png',
  'icon.png',
  'icon.ico',
  'icon.icns',
  'StoreLogo.png',
  'Square30x30Logo.png',
  'Square44x44Logo.png',
  'Square71x71Logo.png',
  'Square89x89Logo.png',
  'Square107x107Logo.png',
  'Square142x142Logo.png',
  'Square150x150Logo.png',
  'Square284x284Logo.png',
  'Square310x310Logo.png',
];

const temp = await mkdtemp(join(tmpdir(), 'knowl-pad-icons-'));
try {
  execFileSync(process.execPath, [tauriCli, 'icon', source, '--output', temp], {
    cwd: root,
    stdio: 'inherit',
  });
  for (const name of desktopFiles) {
    await cp(join(temp, name), join(target, name));
  }
  console.log(`icons updated: ${desktopFiles.length} desktop files -> src-tauri/icons`);
} finally {
  await rm(temp, { recursive: true, force: true });
}