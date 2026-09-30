#!/usr/bin/env node
// scripts/publish-gitee-release.mjs
// 替代已停更的 semantic-release-gitee（last publish 2022-02，违反红线 R-16）。
// 直接调用 Gitee OpenAPI v5 完成「创建/复用 Release + 上传附件」。
//
// 用法：
//   node scripts/publish-gitee-release.mjs --tag v1.0.0 --notes-file CHANGELOG.md --assets "dist/**/*"
//       环境变量：GITEE_TOKEN（必填）、GITEE_OWNER、GITEE_REPO
//       可选：--target <sha|branch>、--prerelease、--dry-run、--timeout <ms>
//
// 设计要点：
//   1. 零第三方依赖（Node >= 18 的 fetch/FormData/Blob）。
//   2. --dry-run 打印全部请求（含 body 预览），用于首次联调与 CI 冒烟。
//   3. 幂等：按 tag 查询已有 Release，存在则复用（不重复创建）。
//   4. 附件重名冲突视为「已存在」跳过，其余错误立即失败（fail fast）。
//   5. 不做任何静默降级：失败即非零退出，并打印 Gitee 返回体便于定位。

import { readFile, stat } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import process from 'node:process';

const API = 'https://gitee.com/api/v5';
const MAX_RETRY = 3;

function parseArgs(argv) {
  const out = { prerelease: false, dryRun: false, timeout: 60000, assets: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error('缺少 ' + a + ' 的值');
      return v;
    };
    switch (a) {
      case '--tag': out.tag = next(); break;
      case '--name': out.name = next(); break;
      case '--notes-file': out.notesFile = next(); break;
      case '--notes': out.notes = next(); break;
      case '--assets': out.assets.push(next()); break;
      case '--target': out.target = next(); break;
      case '--owner': out.owner = next(); break;
      case '--repo': out.repo = next(); break;
      case '--timeout': out.timeout = Number(next()); break;
      case '--prerelease': out.prerelease = true; break;
      case '--dry-run': out.dryRun = true; break;
      case '--verify': out.verify = true; break;
      case '-h': case '--help': out.help = true; break;
      default: throw new Error('未知参数：' + a);
    }
  }
  return out;
}

/** 极简 glob：支持 **、*、?，路径统一为 / 分隔。不引入任何依赖。 */
function globToRegExp(pattern) {
  const norm = pattern.replace(/\\/g, '/');
  let re = '';
  for (let i = 0; i < norm.length; i++) {
    const c = norm[i];
    if (c === '*') {
      if (norm[i + 1] === '*') {
        // ** 匹配任意层级（含 /）
        i++;
        if (norm[i + 1] === '/') i++;
        re += '(?:.*/)?';
      } else {
        re += '[^/]*';
      }
    } else if (c === '?') {
      re += '[^/]';
    } else if ('\\^$.|+()[]{}'.includes(c)) {
      re += '\\' + c;
    } else {
      re += c;
    }
  }
  return new RegExp('^' + re + '$');
}

async function expandAssets(patterns, root) {
  if (!patterns.length) return [];
  const regexes = patterns.map(globToRegExp);
  const found = [];
  async function walk(dir) {
    let entries;
    try { entries = await (await import('node:fs')).promises.readdir(dir, { withFileTypes: true }); }
    catch { return; }
    for (const e of entries) {
      const p = dir + '/' + e.name;
      const rel = p.slice(root.length + 1);
      if (e.isDirectory()) await walk(p);
      else if (regexes.some((r) => r.test(rel))) found.push(p);
    }
  }
  await walk(root);
  return [...new Set(found)].sort();
}

let reported = false;

/** 统一失败出口：先打印，再抛出由 main().catch 收尾。
 *  不再直接 process.exit()——带未关闭的 HTTP socket 硬退出在 Windows 上会 fail-fast（0xC0000409）。 */
function fail(msg, detail) {
  reported = true;
  console.error('❌ ' + msg);
  if (detail !== undefined) console.error(typeof detail === 'string' ? detail : JSON.stringify(detail, null, 2));
  throw new Error(msg);
}

async function request(url, init, timeout, label) {
  for (let attempt = 1; attempt <= MAX_RETRY; attempt++) {
    const ac = new AbortController();
    const timer = setTimeout(() => ac.abort(), timeout);
    try {
      const res = await fetch(url, { ...init, signal: ac.signal });
      clearTimeout(timer);
      const text = await res.text();
      if (res.status >= 500 && attempt < MAX_RETRY) {
        console.error('  ↻ ' + label + ' 第 ' + attempt + ' 次失败（HTTP ' + res.status + '），重试…');
        await new Promise((r) => setTimeout(r, 1500 * attempt));
        continue;
      }
      return { status: res.status, text };
    } catch (err) {
      clearTimeout(timer);
      if (attempt < MAX_RETRY) { await new Promise((r) => setTimeout(r, 1500 * attempt)); continue; }
      throw err;
    }
  }
  throw new Error(label + ' 重试耗尽');
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) { console.log(await readFile(new URL(import.meta.url))); return; }

  const token = process.env.GITEE_TOKEN;
  const owner = args.owner || process.env.GITEE_OWNER;
  const repo = args.repo || process.env.GITEE_REPO;

  if (!args.tag) fail('缺少 --tag');
  if (!args.dryRun && !args.verify && !token) fail('缺少环境变量 GITEE_TOKEN');
  if (!owner || !repo) fail('缺少 owner/repo（--owner/--repo 或 GITEE_OWNER/GITEE_REPO）');

  const notes = args.notesFile ? await readFile(args.notesFile, 'utf8') : (args.notes || '');
  const name = args.name || args.tag;

  // --verify：只读联调——确认端点可达、token 有效、release 是否存在、附件清单
  if (args.verify) {
    const q = token ? '?access_token=' + encodeURIComponent(token) : '';
    const url = API + '/repos/' + owner + '/' + repo + '/releases/tags/' + encodeURIComponent(args.tag) + q;
    const res = await request(url, { method: 'GET' }, args.timeout, '查询 Release');
    if (res.status === 200) {
      const rel = JSON.parse(res.text);
      if (!rel || typeof rel !== 'object') {
        console.log('✅ 端点可达 · 该 tag 尚无 Release（Gitee 对不存在的 tag 返回 200 + null，属正常）');
        process.exitCode = 2;
        return;
      }
      console.log('✅ 端点可达 · Release 已存在：id=' + rel.id + ' tag=' + rel.tag_name + ' name=' + (rel.name || ''));
      const att = await request(API + '/repos/' + owner + '/' + repo + '/releases/' + rel.id + '/attach_files?per_page=100' + (token ? '&access_token=' + encodeURIComponent(token) : ''), { method: 'GET' }, args.timeout, '查询附件');
      if (att.status === 200) {
        const list = JSON.parse(att.text);
        console.log('   已上传附件 ' + list.length + ' 个' + (list.length ? '：' + list.map((a) => a.name).join(', ') : ''));
      }
      process.exitCode = 0;
      return;
    }
    if (res.status === 404) {
      console.log('✅ 端点可达 · 该 tag 尚无 Release（首次发布属正常）');
      process.exitCode = 2;
      return;
    }
    if (res.status === 401 || res.status === 403) {
      console.error('❌ token 无效或权限不足（HTTP ' + res.status + '）；Gitee 私人令牌需勾选 projects 权限');
      process.exitCode = 1;
      return;
    }
    console.error('❌ 意外响应 HTTP ' + res.status, res.text);
    process.exitCode = 1;
    return;
  }

  const root = resolve('.');
  const files = await expandAssets(args.assets, root);
  console.log('发布目标：' + owner + '/' + repo + '  tag=' + args.tag + '  资产 ' + files.length + ' 个' + (args.dryRun ? '  [DRY-RUN]' : ''));

  // 1) 查询已有 Release（幂等）
  const getUrl = API + '/repos/' + owner + '/' + repo + '/releases/tags/' + encodeURIComponent(args.tag) + '?access_token=' + encodeURIComponent(token || '<TOKEN>');
  if (args.dryRun) console.log('→ GET ' + getUrl.replace(token || 'x', '<TOKEN>'));

  let releaseId = null;
  let existed = false;
  if (!args.dryRun) {
    const r = await request(getUrl, { method: 'GET' }, args.timeout, '查询 Release');
    if (r.status === 200) {
      const rel = JSON.parse(r.text);
      if (rel && typeof rel === 'object') {
        releaseId = rel.id;
        existed = true;
        console.log('✓ 已存在 Release id=' + releaseId + '，复用');
      } else {
        console.log('（Gitee 对不存在的 tag 返回 200 + null，按“尚未创建”处理）');
      }
    } else if (r.status !== 404) {
      fail('查询 Release 失败（HTTP ' + r.status + '）', r.text);
    }
  }

  // 2) 创建 Release
  if (!existed) {
    const form = new URLSearchParams({
      access_token: token || '<TOKEN>',
      tag_name: args.tag,
      name,
      body: notes,
    });
    if (args.target) form.set('target_commitish', args.target);
    if (args.prerelease) form.set('prerelease', 'true');
    const createUrl = API + '/repos/' + owner + '/' + repo + '/releases';
    if (args.dryRun) {
      console.log('→ POST ' + createUrl);
      console.log('  form: ' + form.toString().replace(encodeURIComponent('<TOKEN>'), '<TOKEN>').replace(token || 'x', '<TOKEN>'));
    } else {
      const r = await request(createUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: form.toString(),
      }, args.timeout, '创建 Release');
      if (r.status !== 201 && r.status !== 200) fail('创建 Release 失败（HTTP ' + r.status + '）', r.text);
      releaseId = JSON.parse(r.text).id;
      console.log('✓ Release 创建成功 id=' + releaseId);
    }
  }

  // 3) 上传资产
  let uploaded = 0, skipped = 0;
  for (const f of files) {
    const size = (await stat(f)).size;
    const uploadUrl = API + '/repos/' + owner + '/' + repo + '/releases/' + (releaseId ?? '<ID>') + '/attach_files';
    if (args.dryRun) {
      console.log('→ POST ' + uploadUrl + '  file=' + f + ' (' + size + ' bytes)');
      uploaded++;
      continue;
    }
    const buf = await readFile(f);
    const fd = new FormData();
    fd.append('access_token', token);
    fd.append('file', new Blob([buf]), basename(f));
    const r = await request(uploadUrl, { method: 'POST', body: fd }, Math.max(args.timeout, 120000), '上传 ' + basename(f));
    if (r.status === 201 || r.status === 200) {
      uploaded++;
      console.log('  ✓ ' + basename(f) + ' (' + size + ' bytes)');
    } else if (/exist|已存在|already/i.test(r.text)) {
      skipped++;
      console.log('  ↷ ' + basename(f) + ' 已存在，跳过');
    } else {
      fail('上传 ' + basename(f) + ' 失败（HTTP ' + r.status + '）', r.text);
    }
  }

  console.log('完成：' + (args.dryRun ? '[DRY-RUN] ' : '') + '新建资产 ' + uploaded + ' 个，跳过 ' + skipped + ' 个');
}

main().catch((e) => {
  if (!reported) console.error('❌ ' + (e && e.message ? e.message : String(e)));
  process.exitCode = 1;
});
