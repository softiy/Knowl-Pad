#!/usr/bin/env node
// 门禁 12：可靠性测试集（原子写入一致性 / 无半截文件 / 无临时文件残留）。
import { spawnSync } from 'node:child_process';
import process from 'node:process';
console.log('▶ cargo test --test reliability');
const res = spawnSync('cargo', ['test', '-p', 'kp-domain', '--test', 'reliability', '--', '--nocapture'], { stdio: 'inherit', shell: process.platform === 'win32' });
if ((res.status ?? 1) !== 0) { console.error('❌ 可靠性测试集失败'); process.exit(1); }
console.log('✅ 可靠性测试集通过');
