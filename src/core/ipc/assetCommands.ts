//! 本地附件 ⇒ asset 协议 URL（E1 / FR-EDITOR-43 / SEC-08）。
//!
//! 为什么前端不自己拼 `asset://`：Windows 的形态是 `http://asset.localhost/<编码后的绝对路径>`、
//! 其余平台是 `asset://localhost/<编码后的绝对路径>`，编码规则必须与 Rust 侧 asset 协议处理器
//! 完全一致 —— 那是 Tauri 的 `convertFileSrc`，不是我们能手写的字符串模板（手写必错编码）。
//!
//! 为什么先做纯路径解析（`core/markdown/assetPath`）：`convertFileSrc` 只负责"绝对路径 ⇒ URL"，
//! 不做任何范围检查；把相对 src 解析成绝对路径这一步必须由我们控制（拒绝远程/绝对路径/越界），
//! 否则笔记里的 `![x](../../../Windows/x.png)` 会变成一个可请求的 asset URL。
import { convertFileSrc } from '@tauri-apps/api/core';
import { resolveAssetSrc } from '@core/markdown/assetPath';

/** Tauri 运行时暴露的转换函数（浏览器预览 / 单测环境没有它）。 */
interface AssetRuntime {
  convertFileSrc?: (filePath: string, protocol?: string) => string;
}

function assetRuntime(): AssetRuntime | undefined {
  return (globalThis as { __TAURI_INTERNALS__?: AssetRuntime }).__TAURI_INTERNALS__;
}

/**
 * 相对当前笔记的本地图片 src ⇒ 可放进 `<img src>` 的 asset URL；不允许/不可用时 `null`。
 *
 * 返回 `null` 时渲染器保留原 src —— 这比"猜一个 URL"安全：猜出来的 URL 要么指错文件，
 * 要么在 CSP 之外，而 `null` 至少不会引入网络请求（SEC-08）。
 */
export function assetUrlFor(vaultRoot: string, noteRelPath: string, src: string): string | null {
  const absPath = resolveAssetSrc(vaultRoot, noteRelPath, src);
  if (absPath === null) return null;
  const runtime = assetRuntime();
  // 浏览器调试（pnpm dev）与单测环境没有 Tauri 运行时：返回 null，不让整篇渲染抛异常。
  if (typeof runtime?.convertFileSrc !== 'function') return null;
  return convertFileSrc(absPath);
}
