//! 外部链接的 IPC 封装（SEC-07 / SEC-08 / FR-ATTACH-11）。
//!
//! 为什么不用前端 opener/shell 插件直接 open：① FE-04 规定 features 不得直接碰 `@tauri-apps/api`；
//! ② 协议白名单必须在**可信侧**（Rust）执行 —— 前端传来的 URL 来自笔记正文，属于不可信输入，
//! 由前端自行过滤等于没有边界。命令侧只放行 http/https/mailto（见 `commands/system.rs`）。
import { callVoid } from './client';

/**
 * 用系统默认浏览器打开外部 URL（远程图片占位符 / 外链）。
 *
 * 应用进程自身不发起任何网络请求（SEC-08 的"零网络"）：网络请求的发起方是浏览器。
 * 失败时抛 `KpError`（调用方必须给出可见降级，不得空吞，R-15）。
 */
export const openExternal = (url: string): Promise<void> => callVoid('open_external', { url });
