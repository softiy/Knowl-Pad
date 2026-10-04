//! 统一的「跑一个外部工具」实现（门禁运行器与重试包装共用）。
//!
//! 这里每一条约束都是本机踩过的坑，改动前先读 `docs/AGENTS.md` §7 第 4 条：
//! - Windows 上**不经 cmd.exe**：优先在 PATH 里找原生 `.exe`（node 用 process.execPath）；
//! - 输出用 **pipe 捕获后原样回显**，而不是 `stdio: "inherit"`；
//! - 崩溃码 `0xC0000409` 单独识别，是否重试由调用方决定（绝不掩盖真实失败）。

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { delimiter, join } from 'node:path';
import process from 'node:process';

/** 本机沙箱/宿主时钟回拨导致 node 子进程 abort 的退出码（同一值的不同表示）。 */
export const CRASH_CODES = new Set([3221226505, -1073740791, 0xc0000409]);

/** 判断是否为本机环境导致的崩溃码。 */
export function isCrashCode(code) {
  return CRASH_CODES.has(code);
}

/** 为 shell 执行转义单个参数：仅在含空白或 shell 元字符时加引号。 */
export function shellQuote(arg) {
  const s = String(arg);
  return /[\s"&|<>^()%!]/.test(s) ? '"' + s.replace(/"/g, '\\"') + '"' : s;
}

/**
 * 解析命令到可执行文件。
 *
 * Windows 上刻意避免经 cmd.exe：实测经 cmd 启动的 node 会以 libuv 时钟断言崩溃
 * （退出码 0xC0000409）；直连 .exe 则正常。仅当只找到 .cmd/.bat 包装器时才回退到 shell。
 */
export function resolveCommand(cmd) {
  if (process.platform !== 'win32') return { command: cmd, shell: false };
  if (cmd === 'node') return { command: process.execPath, shell: false };
  const dirs = (process.env.PATH ?? '').split(delimiter).filter(Boolean);
  // 先在**全部** PATH 目录里找原生 .exe（避免同目录的 .cmd 抢先命中而被迫经 cmd.exe）
  for (const dir of dirs) {
    const candidate = join(dir, `${cmd}.exe`);
    if (existsSync(candidate)) return { command: candidate, shell: false };
  }
  for (const dir of dirs) {
    for (const ext of ['.cmd', '.bat']) {
      const candidate = join(dir, cmd + ext);
      if (existsSync(candidate)) return { command: candidate, shell: true };
    }
  }
  return { command: cmd, shell: false };
}

/**
 * 运行外部命令；返回 `{ code, ms, printable, attempts }`。
 *
 * @param retries 命中崩溃码时的额外重试次数（默认 0；门禁与测试包装各按需设置）
 * @param echo    是否把捕获到的输出原样回显（默认 true，保证日志可见）
 */
export function spawnTool(cmd, args, { retries = 0, echo = true } = {}) {
  const printable = [cmd, ...args].join(' ');
  const { command, shell } = resolveCommand(cmd);
  const started = Date.now();
  let result;
  let attempt = 0;
  for (attempt = 1; attempt <= retries + 1; attempt += 1) {
    result = shell
      ? spawnSync([command, ...args].map(shellQuote).join(' '), { encoding: 'utf8', shell: true })
      : spawnSync(command, args, { encoding: 'utf8' });
    if (echo) {
      if (result.stdout) process.stdout.write(result.stdout);
      if (result.stderr) process.stderr.write(result.stderr);
    }
    const code = result.status ?? 1;
    if (!isCrashCode(code)) {
      return { code, ms: Date.now() - started, printable, attempts: attempt };
    }
    if (attempt <= retries) {
      console.error(`⚠️ 子进程被本机环境打断（0x${(code >>> 0).toString(16)}），重试 ${attempt}/${retries}`);
    }
  }
  return { code: result?.status ?? 1, ms: Date.now() - started, printable, attempts: attempt - 1 };
}
