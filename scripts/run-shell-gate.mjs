#!/usr/bin/env node
// 运行 §11.7.8 的 shell 门禁脚本。在 Windows 上自动为 WSL/Git-Bash 准备 rg 包装器，
// 使 native rg.exe 能被 `command -v rg` 找到；缺少 bash/ripgrep 时以退出码 2 硬失败。
import { spawnSync } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import process from 'node:process';

const MAP = {
  'check-naming': 'scripts/check-naming.sh',
  'check-path-encapsulation': 'scripts/check-path-encapsulation.sh',
};
const which = process.argv[2];
const script = MAP[which];
if (!script) { console.error('用法：node scripts/run-shell-gate.mjs <check-naming|check-path-encapsulation>'); process.exit(2); }

const bashOk = spawnSync('bash', ['--version'], { stdio: 'ignore' }).status === 0;
if (!bashOk) { console.error('❌ 未找到 bash，无法运行 shell 门禁'); process.exit(2); }

// 把 Windows 路径转为 WSL 可见路径
function toPosix(p) {
  const m = /^([A-Za-z]):[\\/](.*)$/.exec(p);
  return m ? '/mnt/' + m[1].toLowerCase() + '/' + m[2].replace(/\\/g, '/') : p.replace(/\\/g, '/');
}

// 关键：准备 rg 包装器与执行门禁脚本必须在**同一次 bash 调用**内完成——
// WSL 会在两次调用之间关闭实例并清空 /tmp，跨调用会丢失包装器。
let bashCmd;
const rgInside = spawnSync('bash', ['-c', 'command -v rg'], { encoding: 'utf8' });
if (rgInside.status !== 0) {
  const localRg = join(process.cwd(), 'tools', 'rg.exe');
  if (!existsSync(localRg)) {
    console.error('❌ 缺少 ripgrep（rg）。CI 用 choco/apt 安装；本地可放入 tools/rg.exe');
    process.exit(2);
  }
  const wrapperWin = join(process.cwd(), 'tools', 'rg-wrapper');
  writeFileSync(wrapperWin, '#!/bin/sh\nexec "' + toPosix(localRg) + '" "$@"\n', 'utf8');
  bashCmd = 'mkdir -p /tmp/kpbin && cp "' + toPosix(wrapperWin) + '" /tmp/kpbin/rg && chmod +x /tmp/kpbin/rg && export PATH="/tmp/kpbin:$PATH" && bash "' + script + '"';
  console.log('（已在同一次 bash 会话内准备 rg 包装器）');
} else {
  bashCmd = 'bash ' + script;
}

const res = spawnSync('bash', ['-c', bashCmd], { stdio: 'inherit' });
process.exit(res.status ?? 1);
