#!/usr/bin/env node
//! 只在「本机环境崩溃码（0xC0000409）」上重试的命令包装。
//!
//! 用法：`node scripts/run-with-retry.mjs <命令> [参数...]`
//! 语义保证：**只**对崩溃码重试；其它非零退出码原样透传（绝不把真实失败"重试成绿"），
//! 且每次重试都会在 stderr 打印次数。背景见 `docs/AGENTS.md` §7 第 4 条。

import process from 'node:process';
import { isCrashCode, spawnTool } from './lib/spawn-tool.mjs';

/** 最多尝试次数（首次 + 2 次重试）。 */
const MAX_ATTEMPTS = 3;

const [cmd, ...args] = process.argv.slice(2);
if (!cmd) {
  console.error('用法：node scripts/run-with-retry.mjs <命令> [参数...]');
  process.exit(2);
}

let last = { code: 1, attempts: 0, printable: cmd };
for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt += 1) {
  const result = spawnTool(cmd, args, { retries: 0 });
  last = result;
  if (!isCrashCode(result.code)) {
    if (attempt > 1 && result.code === 0) {
      console.error(`✅ 第 ${attempt} 次尝试成功（前 ${attempt - 1} 次被本机环境打断）`);
    }
    process.exit(result.code);
  }
  console.error(
    `⚠️ ${result.printable} 命中本机环境崩溃码 0x${(result.code >>> 0).toString(16)}` +
      `（libuv 时钟断言，见 docs/AGENTS.md §7 第 4 条）：第 ${attempt}/${MAX_ATTEMPTS} 次`
  );
}

console.error(`❌ ${last.printable} 连续 ${MAX_ATTEMPTS} 次被本机环境打断，放弃（非代码问题，可稍后重跑或交给 CI）`);
process.exit(last.code);
