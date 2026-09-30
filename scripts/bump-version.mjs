#!/usr/bin/env node
// scripts/bump-version.mjs <version>
// 把版本号同步到四处：package.json、src-tauri/tauri.conf.json、src-tauri/Cargo.toml、以及（由 release tag 体现的）Git tag。
// 由 semantic-release 的 @semantic-release/exec prepareCmd 调用；禁止手工改动其中任何一处。
import { readFile, writeFile } from 'node:fs/promises';
import process from 'node:process';

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error('用法：node scripts/bump-version.mjs <semver>');
  process.exit(1);
}

const files = {
  'package.json': {
    get: (s) => JSON.parse(s),
    set: (o, v) => { o.version = v; },
    dump: (o) => JSON.stringify(o, null, 2) + '\n',
  },
  'src-tauri/tauri.conf.json': {
    get: (s) => JSON.parse(s),
    set: (o, v) => { o.version = v; },
    dump: (o) => JSON.stringify(o, null, 2) + '\n',
  },
  'src-tauri/Cargo.toml': {
    get: (s) => s,
    set: (o, v) => o.replace(/(\[package\][\s\S]*?^version\s*=\s*)"[^"]*"/m, '$1"' + v + '"'),
    dump: (o) => o,
  },
};

let changed = 0;
for (const [file, io] of Object.entries(files)) {
  const raw = await readFile(file, 'utf8');
  const obj = io.get(raw);
  io.set(obj, version);
  const next = io.dump(obj);
  if (next !== raw) { await writeFile(file, next, 'utf8'); changed++; console.log('  ✓ ' + file + ' → ' + version); }
  else console.log('  = ' + file + ' 已是 ' + version);
}

// 一致性校验：四处（含 tag）必须一致
const pkg = JSON.parse(await readFile('package.json', 'utf8')).version;
const tauri = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8')).version;
const cargo = (await readFile('src-tauri/Cargo.toml', 'utf8')).match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
if (pkg !== tauri || pkg !== cargo) {
  console.error('❌ 版本号不一致：package.json=' + pkg + ' tauri.conf.json=' + tauri + ' Cargo.toml=' + cargo);
  process.exit(1);
}
console.log('✅ 版本号已同步为 ' + pkg + '（package.json / tauri.conf.json / Cargo.toml；tag 由 CI 校验）');
