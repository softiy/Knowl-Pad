#!/usr/bin/env node
// 发布准备编排（DEBT-11）：推断版本 → 同步四处版本 + 生成 CHANGELOG → 打印后续命令。
// 默认 dry-run；--write 才落盘。要求工作区干净（除非 --allow-dirty）。
import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import process from 'node:process';
import { bumpVersion, groupCommits, inferBump, parseLog, prependSection, renderChangelog } from './changelog.mjs';

function parseArgs(argv) {
  const out = { write: false, allowDirty: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--write') out.write = true;
    else if (a === '--allow-dirty') out.allowDirty = true;
    else if (a === '--help') out.help = true;
    else if (a.startsWith('--')) out[a.slice(2)] = argv[++i];
  }
  return out;
}
const git = (args) => execFileSync('git', args, { encoding: 'utf8' });

function lastTag() {
  try { return git(['describe', '--tags', '--abbrev=0']).trim(); } catch { return null; }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) { console.log(await readFile(new URL(import.meta.url), 'utf8')); return; }

  const dirty = git(['status', '--porcelain']).trim();
  if (dirty && !args.allowDirty) {
    console.error('❌ 工作区不干净，拒绝生成发布提交（用 --allow-dirty 可跳过）：');
    console.error(dirty.split('\n').slice(0, 10).map((l) => '   ' + l).join('\n'));
    process.exitCode = 1;
    return;
  }

  const from = args.from ?? lastTag();
  const to = args.to ?? 'HEAD';
  const range = from ? `${from}..${to}` : to;
  const commits = parseLog(git(['log', '--pretty=format:%H%x1f%s%x1f%b%x1e', range]));
  const { groups, breaking } = groupCommits(commits);
  const kind = inferBump(commits);
  const pkgVersion = JSON.parse(await readFile('package.json', 'utf8')).version;
  const version = args.version ?? (kind ? bumpVersion(pkgVersion, kind) : pkgVersion);
  const tag = `v${version}`;

  console.log('== 发布准备 ==');
  console.log(`区间      : ${from ?? '(首个提交)'}..${to}`);
  console.log(`提交      : ${commits.length} 条（可见 ${[...groups.values()].reduce((n, v) => n + v.length, 0)} 条，破坏性 ${breaking.length} 条）`);
  console.log(`递增      : ${kind ?? '无'}`);
  console.log(`版本      : ${pkgVersion} → ${version}`);
  console.log(`将改动    : package.json / src-tauri/tauri.conf.json / src-tauri/Cargo.toml / CHANGELOG.md`);
  console.log(`${args.write ? '' : '[dry-run] '}标签      : ${tag}`);
  const section = renderChangelog({ version, date: new Date().toISOString().slice(0, 10), groups, breaking, from, to, repoUrl: args.repo ?? null });
  console.log('\n---- CHANGELOG 预览 ----\n' + section);

  if (!args.write) {
    console.log('（dry-run；加 --write 才会同步版本并写入 CHANGELOG.md）');
    return;
  }
  execFileSync('node', ['scripts/bump-version.mjs', version], { stdio: 'inherit' });
  const existing = await readFile('CHANGELOG.md', 'utf8').catch(() => '# 更新日志\n');
  await writeFile('CHANGELOG.md', prependSection(existing, section), 'utf8');
  console.log(`\n✅ 已同步版本并写入 CHANGELOG.md\n下一步：\n  git add -A && git commit -m "chore(release): ${version}"\n  git tag ${tag}\n  git push --follow-tags`);
}

main().catch((err) => { console.error('❌ ' + (err && err.message ? err.message : String(err))); process.exitCode = 1; });
