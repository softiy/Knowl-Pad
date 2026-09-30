#!/usr/bin/env node
// 依据 Conventional Commits 生成 CHANGELOG（替代已删除的 semantic-release 配置，见 DEBT-11）。
// 设计：零依赖、纯函数可单测；只读 git 历史，不写文件（除非 --write）。
//
// 用法：
//   node scripts/changelog.mjs                      # 打印「上一个 tag → HEAD」的变更（dry-run）
//   node scripts/changelog.mjs --from <ref> --to <ref>
//   node scripts/changelog.mjs --version 1.2.0 --write   # 写入 CHANGELOG.md 顶部
//   node scripts/changelog.mjs --self-test          # 内置自检（不需要 git 仓库）

import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import process from 'node:process';

/** 可见的分组（其余类型归入维护，不进入 CHANGELOG 正文）。 */
const VISIBLE_TYPES = {
  feat: '✨ 新功能',
  fix: '🐛 问题修复',
  perf: '⚡ 性能优化',
  revert: '⏪ 回滚',
};

/** 解析 Conventional Commits 头部；不符合规范返回 null。 */
export function parseHeader(header) {
  const m = /^(?<type>[a-z]+)(?:\((?<scope>[^)]+)\))?(?<bang>!)?:\s*(?<subject>.+)$/.exec(
    String(header ?? '').trim(),
  );
  if (!m) return null;
  return {
    type: m.groups.type,
    scope: m.groups.scope ?? null,
    bang: Boolean(m.groups.bang),
    subject: m.groups.subject,
  };
}

/** 解析 git log 原始输出（记录以 \x1e 分隔、字段以 \x1f 分隔：hash, header, body）。 */
export function parseLog(raw) {
  return String(raw ?? '')
    .split('\x1e')
    .map((r) => r.trim())
    .filter(Boolean)
    .map((record) => {
      const [hash, header, body = ''] = record.split('\x1f');
      const parsed = parseHeader(header);
      if (!parsed) return null;
      const breaking = parsed.bang || /^BREAKING CHANGE:/m.test(body);
      return { hash: String(hash ?? '').slice(0, 7), ...parsed, breaking, body };
    })
    .filter((c) => c !== null)
    // 过滤发布提交：scope 为 release 的 chore（如 chore(release): 1.2.0），以及标题含 [skip ci] 的提交
    .filter((c) => !(c.type === 'chore' && (c.scope === 'release' || /\[skip ci\]/.test(c.subject))));
}

/** 分组：可见类型进入对应小节，破坏性变更单独汇总。 */
export function groupCommits(commits) {
  const groups = new Map();
  const breaking = [];
  for (const c of commits) {
    if (c.breaking) breaking.push(c);
    const title = VISIBLE_TYPES[c.type];
    if (!title) continue;
    if (!groups.has(title)) groups.set(title, []);
    groups.get(title).push(c);
  }
  return { groups, breaking };
}

/** 版本推断：破坏性 → major；feat → minor；其余有提交 → patch；无提交 → null。 */
export function inferBump(commits) {
  if (commits.some((c) => c.breaking)) return 'major';
  if (commits.some((c) => c.type === 'feat')) return 'minor';
  return commits.length > 0 ? 'patch' : null;
}

/** 语义化版本递增。 */
export function bumpVersion(version, kind) {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec(String(version ?? ''));
  if (!m) throw new Error('非法版本号：' + version);
  const [major, minor, patch] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (kind === 'major') return `${major + 1}.0.0`;
  if (kind === 'minor') return `${major}.${minor + 1}.0`;
  if (kind === 'patch') return `${major}.${minor}.${patch + 1}`;
  throw new Error('未知递增类型：' + kind);
}

/** 渲染一段 Keep-a-Changelog 风格的小节。 */
export function renderChangelog({ version, date, groups, breaking, from, to, repoUrl }) {
  const lines = [`## [${version}] - ${date}`, ''];
  const link = repoUrl && from && to ? `（[${from}...${to}](${repoUrl}/compare/${from}...${to})）` : '';
  if (link) lines.push(link, '');
  if (groups.size === 0) lines.push('本版本无面向用户的变更。', '');
  for (const [title, items] of groups) {
    lines.push(`### ${title}`, '');
    for (const c of items) {
      const scope = c.scope ? `**${c.scope}**: ` : '';
      lines.push(`- ${scope}${c.subject} (${c.hash})`);
    }
    lines.push('');
  }
  if (breaking.length > 0) {
    lines.push('### ⚠️ 破坏性变更', '');
    for (const c of breaking) lines.push(`- ${c.subject} (${c.hash})`);
    lines.push('');
  }
  return lines.join('\n');
}

/** 把新小节插入到既有 CHANGELOG 的首个一级标题之后。 */
export function prependSection(existing, section) {
  const text = String(existing ?? '');
  const firstBreak = text.indexOf('\n');
  if (firstBreak < 0) return text.trimEnd() + '\n\n' + section;
  return text.slice(0, firstBreak + 1) + '\n' + section + text.slice(firstBreak + 1);
}

function git(args, { quiet = false } = {}) {
  // quiet：无 tag 时 `git describe` 会向 stderr 输出 fatal，这里吞掉，避免污染 changelog 输出
  return execFileSync('git', args, { encoding: 'utf8', stdio: quiet ? ['ignore', 'pipe', 'ignore'] : ['ignore', 'pipe', 'inherit'] });
}

function lastTag() {
  try {
    return git(['describe', '--tags', '--abbrev=0'], { quiet: true }).trim();
  } catch {
    return null;
  }
}

function readLog(from, to) {
  const range = from ? `${from}..${to}` : to;
  return git(['log', `--pretty=format:%H%x1f%s%x1f%b%x1e`, range]);
}

function parseArgs(argv) {
  const out = { write: false, selfTest: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--write') out.write = true;
    else if (a === '--self-test') out.selfTest = true;
    else if (a === '--help') out.help = true;
    else if (a.startsWith('--')) out[a.slice(2)] = argv[++i];
  }
  return out;
}

/** 内置自检：不依赖 git 仓库，验证解析/分组/推断/渲染。 */
export function selfTest() {
  const raw = [
    'a1b2c3d\x1ffeat(editor): 支持 wikilink 补全\x1f\x1e',
    'b2c3d4e\x1ffix: 修正重命名时的悬空链接\x1f\x1e',
    'c3d4e5f\x1fchore(deps): 升级 vite\x1f\x1e',
    'd4e5f6a\x1fdocs: 修正错别字\x1f\x1e',
    'e5f6a7b\x1frefactor(link)!: 改写器改为两阶段\x1fBREAKING CHANGE: 预览与执行分离\x1e',
    'f6a7b8c\x1fchore(release): 1.0.0 [skip ci]\x1f\x1e',
  ].join('');
  const commits = parseLog(raw);
  const { groups, breaking } = groupCommits(commits);
  const problems = [];
  if (commits.length !== 5) problems.push('应过滤 release 提交，得到 5 条，实得 ' + commits.length);
  if (inferBump(commits) !== 'major') problems.push('应为 major');
  if (breaking.length !== 1) problems.push('应识别 1 条破坏性变更');
  if (!groups.has('✨ 新功能') || !groups.has('🐛 问题修复')) problems.push('分组缺失');
  if (groups.has('🔧 其他')) problems.push('docs/chore 不应进入正文');
  const text = renderChangelog({ version: '2.0.0', date: '2026-09-30', groups, breaking });
  if (!text.includes('## [2.0.0] - 2026-09-30')) problems.push('渲染缺少标题');
  if (!text.includes('⚠️ 破坏性变更')) problems.push('渲染缺少破坏性变更小节');
  if (bumpVersion('1.2.3', 'minor') !== '1.3.0') problems.push('minor 递增错误');
  if (prependSection('# 日志\n\n旧内容\n', '## [1.0.0]\n') .indexOf('## [1.0.0]') > 12) problems.push('插入位置错误');
  if (problems.length) {
    for (const p of problems) console.error('❌ ' + p);
    process.exitCode = 1;
    return false;
  }
  console.log('✅ changelog 自检通过（解析/分组/推断/渲染/插入 共 9 项断言）');
  return true;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(await readFile(new URL(import.meta.url), 'utf8'));
    return;
  }
  if (args.selfTest) {
    selfTest();
    return;
  }
  const to = args.to ?? 'HEAD';
  const from = args.from ?? lastTag();
  const commits = parseLog(readLog(from, to));
  const { groups, breaking } = groupCommits(commits);
  if (commits.length === 0) {
    console.log('（区间 ' + (from ?? '起始') + '..' + to + ' 内没有符合条件的提交）');
    return;
  }
  const kind = inferBump(commits);
  const pkgVersion = JSON.parse(await readFile('package.json', 'utf8')).version;
  const version = args.version ?? (kind ? bumpVersion(pkgVersion, kind) : pkgVersion);
  const section = renderChangelog({
    version,
    date: new Date().toISOString().slice(0, 10),
    groups,
    breaking,
    from,
    to,
    repoUrl: args.repo ?? null,
  });
  console.log('提交 ' + commits.length + ' 条 | 建议递增：' + (kind ?? '无') + ' | 版本：' + pkgVersion + ' → ' + version);
  console.log('————————————————————————————————');
  console.log(section);
  if (args.write) {
    const existing = await readFile('CHANGELOG.md', 'utf8').catch(() => '# 更新日志\n');
    await writeFile('CHANGELOG.md', prependSection(existing, section), 'utf8');
    console.log('✅ 已写入 CHANGELOG.md');
  } else {
    console.log('（dry-run；加 --write 才会写入 CHANGELOG.md）');
  }
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  main().catch((err) => {
    console.error('❌ ' + (err && err.message ? err.message : String(err)));
    process.exitCode = 1;
  });
}
