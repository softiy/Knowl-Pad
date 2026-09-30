#!/usr/bin/env node
// PR 流程工具：把「分支 → 推送 → 开 PR → 等必需检查 → 合并 → 同步」固化成一条可续跑的命令。
//
// 设计要点：
//   1. 令牌不落地：通过 `git credential fill` 向系统凭据管理器索取，只在内存中使用，永不打印、永不写文件；
//   2. 可续跑：状态写入 .git/pr-flow-state.json（分支/PR 号/head SHA），断网或中断后重跑自动接着做；
//   3. 断网容错：所有 API 调用带指数退避重试；轮询期间连续失败只提示、不中断；
//   4. 合并门槛与 GitHub 分支保护一致：必需检查全部 success 且 mergeable_state=clean 才合并；
//   5. 拒绝在 main 上运行（本项目禁止直推 main，PRD CODE-09）。

import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import process from 'node:process';

/** 与 .github/workflows/ci.yml 的 job 名一一对应，必须与分支保护的必需检查保持一致。 */
export const REQUIRED_CHECKS = ['17 项门禁（与本地同一脚本）', 'Windows 冒烟（构建 + 单元测试）'];
const API = 'https://api.github.com';
const STATE_FILE = '.git/pr-flow-state.json';
const OK_CONCLUSIONS = ['success', 'neutral', 'skipped'];

/** 判定必需检查是否全部通过。纯函数，便于单测。 */
export function evaluateChecks(checkRuns = [], required = REQUIRED_CHECKS) {
  const byName = new Map(checkRuns.map((c) => [c.name, c]));
  const missing = required.filter((name) => !byName.has(name));
  const present = required.filter((name) => byName.has(name));
  const pending = present.filter((name) => byName.get(name).status !== 'completed');
  const failed = present.filter((name) => {
    const c = byName.get(name);
    return c.status === 'completed' && !OK_CONCLUSIONS.includes(c.conclusion);
  });
  const unknown = checkRuns.filter((c) => !required.includes(c.name)).map((c) => c.name);
  return { ok: missing.length === 0 && pending.length === 0 && failed.length === 0, missing, pending, failed, unknown };
}

/** 指数退避（毫秒），上限 20s。纯函数。 */
export function backoffMs(attempt, base = 1500, cap = 20000) {
  return Math.min(cap, base * Math.pow(2, Math.max(0, attempt - 1)));
}

/** 从 git remote URL 解析出 owner/repo（支持 https 与 ssh 两种写法）。纯函数。 */
export function parseRepoFromUrl(url) {
  if (!url) return null;
  const https = /github\.com[/:]([^/]+)\/([^/]+?)(?:\.git)?$/.exec(url.trim());
  if (https) return https[1] + '/' + https[2];
  const gitee = /gitee\.com[/:]([^/]+)\/([^/]+?)(?:\.git)?$/.exec(url.trim());
  if (gitee) return gitee[1] + '/' + gitee[2];
  return null;
}

/** 参数解析。纯函数。 */
export function parseArgs(argv) {
  const opts = { command: 'run', wait: 20, interval: 20, retries: 5, merge: true, mirror: true, verifyMirror: true };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--wait') opts.wait = Number(argv[++i]);
    else if (a === '--interval') opts.interval = Number(argv[++i]);
    else if (a === '--retries') opts.retries = Number(argv[++i]);
    else if (a === '--title') opts.title = argv[++i];
    else if (a === '--body-file') opts.bodyFile = argv[++i];
    else if (a === '--base') opts.base = argv[++i];
    else if (a === '--no-merge') opts.merge = false;
    else if (a === '--no-mirror') opts.verifyMirror = false;
    else if (a === '--help' || a === '-h') opts.help = true;
    else if (a.startsWith('--')) throw new Error('未知参数：' + a);
    else rest.push(a);
  }
  if (rest.length > 0) opts.command = rest[0];
  return opts;
}

// ── 基础设施 ────────────────────────────────────────────────

function git(args, { allowFail = false } = {}) {
  try {
    return execFileSync('git', args, { encoding: 'utf8' }).trim();
  } catch (err) {
    if (allowFail) return null;
    throw new Error('git ' + args.join(' ') + ' 失败：' + (err.stderr || err.message));
  }
}

let cachedCred = null;
/** 取 GitHub 凭据（来自系统凭据管理器），只驻留内存。 */
function credential() {
  if (cachedCred) return cachedCred;
  const out = execFileSync('git', ['credential', 'fill'], {
    encoding: 'utf8',
    input: 'protocol=https\nhost=github.com\n\n',
  });
  const user = /^username=(.*)$/m.exec(out)?.[1] ?? '';
  const pass = /^password=(.*)$/m.exec(out)?.[1] ?? '';
  if (!pass) throw new Error('未能从凭据管理器取得 GitHub 令牌');
  cachedCred = { user, pass };
  return cachedCred;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * 识别 TLS 证书校验失败并给出可操作提示。
 * 背景：本机若使用代理/加速器（TLS 中间人 + 自签 CA），Node 内置信任库不认它，
 * 表现为 fetch failed + cause.code=UNABLE_TO_VERIFY_LEAF_SIGNATURE；而 git 走系统证书库所以正常。
 */
export function tlsHint(err) {
  const code = err && err.cause && err.cause.code ? String(err.cause.code) : '';
  if (!/CERT|UNABLE_TO_VERIFY|SELF_SIGNED|_SSL/i.test(code)) return null;
  return '检测到 TLS 证书校验失败（' + code + '）：本机可能存在代理/加速器的自签证书。' +
    '解决方式：用 pnpm pr（已在 package.json 里带 --use-system-ca），或手工执行 ' +
    'node --use-system-ca scripts/pr-flow.mjs ...，或设置 NODE_EXTRA_CA_CERTS 指向该 CA。';
}

/** 判定 git 失败是否属于「网络类」错误（可安全重试）。纯函数。 */
export function isNetworkError(text) {
  return [
    /unable to access/i,
    /\b50[234]\b/,
    /Could not resolve/i,
    /Connection (was )?reset/i,
    /timed out/i,
    /RPC failed/i,
    /early EOF/i,
    /remote end hung up/i,
    /TLS|SSL/i,
  ].some((re) => re.test(String(text ?? '')));
}

/**
 * 带重试的 git 网络操作（push/pull/fetch）。
 * 只对网络类错误重试；保护分支拒绝（GH006）、非快进等业务错误立即抛出，避免掩盖真实问题。
 */
async function gitNet(args, { retries = 5, label = 'git ' + args[0] } = {}) {
  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      return git(args);
    } catch (err) {
      const text = String(err && err.message ? err.message : err);
      if (!isNetworkError(text) || attempt >= retries) throw err;
      const wait = backoffMs(attempt);
      console.log('  ⚠️ ' + label + ' 失败（第 ' + attempt + '/' + retries + ' 次，网络类错误）：' + text.split('\n')[0].slice(0, 90));
      console.log('     ' + Math.round(wait / 1000) + 's 后重试（可随时 Ctrl+C，重跑本命令会从中断处继续）…');
      await sleep(wait);
    }
  }
}

/** 带指数退避的 API 调用；网络错误自动重试，HTTP 错误原样返回。 */
async function api(path, { method = 'GET', body, retries = 5, raw = false } = {}) {
  const { user, pass } = credential();
  const auth = 'Basic ' + Buffer.from(user + ':' + pass).toString('base64');
  let lastErr = null;
  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      const res = await fetch(API + path, {
        method,
        headers: { Authorization: auth, Accept: 'application/vnd.github+json', 'User-Agent': 'kp-pr-flow', 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(30000),
      });
      if (raw) return res;
      const text = await res.text();
      const json = text ? JSON.parse(text) : null;
      if (!res.ok && res.status !== 405 && res.status !== 409) {
        const msg = json && json.message ? json.message : text.slice(0, 200);
        throw Object.assign(new Error('GitHub API ' + res.status + '：' + msg), { status: res.status, httpError: true });
      }
      return { status: res.status, json };
    } catch (err) {
      if (err.httpError) throw err; // 业务错误不重试
      lastErr = err;
      if (attempt < retries) {
        const wait = backoffMs(attempt);
        console.log('  ⚠️ 网络异常（第 ' + attempt + '/' + retries + ' 次）：' + err.message.slice(0, 80) + '，' + Math.round(wait / 1000) + 's 后重试…');
        await sleep(wait);
      }
    }
  }
  const hint = tlsHint(lastErr);
  throw new Error('网络请求连续失败 ' + retries + ' 次：' + (lastErr && lastErr.message) + (hint ? '\n    ' + hint : ''));
}

async function readState() {
  try { return JSON.parse(await readFile(STATE_FILE, 'utf8')); } catch { return {}; }
}
async function writeState(patch) {
  const next = { ...(await readState()), ...patch, updatedAt: new Date().toISOString() };
  await writeFile(STATE_FILE, JSON.stringify(next, null, 2), 'utf8');
  return next;
}

// ── 业务动作 ────────────────────────────────────────────────

function repoSlug(opts) {
  if (opts.repo) return opts.repo;
  const url = git(['remote', 'get-url', 'origin'], { allowFail: true });
  const slug = parseRepoFromUrl(url);
  if (!slug) throw new Error('无法从 origin 解析仓库，请用 --repo owner/name');
  return slug;
}

async function ensureOpenPr(opts, slug, branch, headSha) {
  const state = await readState();
  if (state.pr) {
    const cur = await api('/repos/' + slug + '/pulls/' + state.pr);
    if (cur.status === 200 && cur.json.state === 'open') {
      console.log('  复用已有 PR #' + state.pr + '（' + cur.json.html_url + '）');
      return cur.json;
    }
  }
  const list = await api('/repos/' + slug + '/pulls?state=open&head=' + slug.split('/')[0] + ':' + encodeURIComponent(branch));
  if (list.status === 200 && Array.isArray(list.json) && list.json.length > 0) {
    const pr = list.json[0];
    console.log('  找到已存在的 PR #' + pr.number + '（' + pr.html_url + '）');
    await writeState({ pr: pr.number, branch, headSha });
    return pr;
  }
  const body = opts.bodyFile ? await readFile(opts.bodyFile, 'utf8') : '';
  const title = opts.title ?? git(['log', '-1', '--pretty=%s']);
  const created = await api('/repos/' + slug + '/pulls', {
    method: 'POST',
    body: { title, head: branch, base: opts.base ?? 'main', body },
    retries: opts.retries,
  });
  if (created.status !== 201) throw new Error('创建 PR 失败（HTTP ' + created.status + '）');
  console.log('  ✅ 已创建 PR #' + created.json.number + '（' + created.json.html_url + '）');
  await writeState({ pr: created.json.number, branch, headSha, base: opts.base ?? 'main' });
  return created.json;
}

async function waitForChecks(opts, slug, prNumber, headSha) {
  const minutes = Number(opts.wait);
  if (!minutes) return { ok: true, skipped: true };
  const deadline = Date.now() + minutes * 60000;
  let networkFailures = 0;
  for (;;) {
    let runs = null;
    try {
      const res = await api('/repos/' + slug + '/commits/' + headSha + '/check-runs', { retries: 2 });
      if (res.status === 200) { runs = res.json.check_runs ?? []; networkFailures = 0; }
    } catch (err) {
      networkFailures++;
      console.log('  ⚠️ 查询检查状态失败（连续 ' + networkFailures + ' 次）：' + err.message.slice(0, 70));
    }
    if (runs) {
      const verdict = evaluateChecks(runs);
      const summary = REQUIRED_CHECKS
        .map((n) => {
          const c = runs.find((x) => x.name === n);
          return n.slice(0, 12) + '=' + (c ? (c.status === 'completed' ? c.conclusion : c.status) : '未出现');
        })
        .join('  ');
      console.log('  ' + new Date().toTimeString().slice(0, 8) + '  ' + summary);
      if (verdict.ok) return { ok: true, verdict };
      if (verdict.failed.length > 0) return { ok: false, verdict, reason: '检查失败：' + verdict.failed.join(', ') };
    }
    if (Date.now() > deadline) return { ok: false, reason: '等待超时（' + minutes + ' 分钟）' };
    await sleep(Math.max(5, Number(opts.interval)) * 1000);
  }
}

async function mergePr(opts, slug, prNumber) {
  const res = await api('/repos/' + slug + '/pulls/' + prNumber + '/merge', { method: 'PUT', body: { merge_method: 'rebase' }, retries: 3 });
  if (res.status === 200 && res.json.merged) {
    console.log('  ✅ 已合并（rebase）→ ' + res.json.sha.slice(0, 7));
    return res.json.sha;
  }
  throw new Error('合并被拒（HTTP ' + res.status + '）：' + (res.json && res.json.message ? res.json.message : '未知原因'));
}

async function syncMain(branch, retries = 5) {
  git(['switch', 'main']);
  await gitNet(['pull', '--ff-only', 'origin', 'main'], { retries, label: '拉取 main' });
  if (branch && branch !== 'main') git(['branch', '-D', branch], { allowFail: true });
  console.log('  ✅ 本地 main 已同步，分支 ' + branch + ' 已删除');
}

async function verifyMirror(slug) {
  const url = git(['remote', 'get-url', 'gitee'], { allowFail: true });
  const gitee = parseRepoFromUrl(url);
  if (!gitee) { console.log('  （未配置 gitee 远端，跳过镜像校验）'); return; }
  const head = git(['rev-parse', 'main']);
  const deadline = Date.now() + 5 * 60000;
  for (;;) {
    let sha = null;
    try {
      const res = await fetch('https://gitee.com/api/v5/repos/' + gitee + '/branches/main', { signal: AbortSignal.timeout(20000) });
      if (res.ok) sha = (await res.json()).commit.sha;
    } catch { /* 忽略，继续轮询 */ }
    if (sha === head) { console.log('  ✅ Gitee 镜像已同步（' + gitee + '）'); return; }
    if (Date.now() > deadline) { console.log('  ⚠️ Gitee 镜像 5 分钟内未同步，请查看 Gitee「仓库镜像管理」的最近状态'); return; }
    await sleep(10000);
  }
}

// ── 命令编排 ────────────────────────────────────────────────

function help() {
  console.log([
    '用法：node scripts/pr-flow.mjs [命令] [选项]',
    '',
    '命令：',
    '  run     默认。推送当前分支 → 开 PR（幂等）→ 等必需检查 → 合并 → 同步本地 main（并校验 Gitee 镜像）',
    '  open    只推送分支并创建/复用 PR，不等待、不合并',
    '  status  打印当前 PR 与必需检查状态',
    '  merge   等待检查全绿后合并（不推送、不开 PR）',
    '  sync    切回 main 并快进（不合并）',
    '',
    '选项：',
    '  --title <标题>        PR 标题（默认取最新提交标题）',
    '  --body-file <路径>    PR 正文文件（推荐复用 .github/pull_request_template.md）',
    '  --base <分支>         目标分支（默认 main）',
    '  --wait <分钟>         等待检查的上限（默认 20；0 表示不等待）',
    '  --interval <秒>       轮询间隔（默认 20）',
    '  --retries <次数>      单次 API 调用的网络重试上限（默认 5）',
    '  --no-merge            只等到检查结果，不自动合并',
    '  --no-mirror           合并后不校验 Gitee 镜像',
    '  --repo <owner/name>   覆盖 origin 解析结果',
    '',
    '特性：断网自动重试；中断后重跑会复用 .git/pr-flow-state.json 中的 PR，不会重复创建。',
  ].join('\n'));
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) { help(); return; }

  const branch = git(['rev-parse', '--abbrev-ref', 'HEAD']);
  const slug = repoSlug(opts);
  const headSha = git(['rev-parse', 'HEAD']);
  const state = await readState();
  const prNumber = state.branch === branch ? state.pr : undefined;

  console.log('== PR 流程 ==');
  console.log('  仓库   : ' + slug);
  console.log('  分支   : ' + branch + '  →  ' + (opts.base ?? state.base ?? 'main'));
  console.log('  提交   : ' + headSha.slice(0, 7) + (prNumber ? '   已有 PR #' + prNumber : ''));

  if (opts.command === 'status') {
    if (!prNumber) { console.log('  当前分支没有记录在案的 PR。'); return; }
    const pr = await api('/repos/' + slug + '/pulls/' + prNumber);
    const runs = await api('/repos/' + slug + '/commits/' + pr.json.head.sha + '/check-runs');
    console.log('  PR #' + pr.json.number + '  ' + pr.json.state + '  ' + pr.json.html_url);
    for (const name of REQUIRED_CHECKS) {
      const c = (runs.json.check_runs ?? []).find((x) => x.name === name);
      console.log('    ' + name + ' → ' + (c ? c.status + '/' + (c.conclusion ?? '-') : '未出现'));
    }
    return;
  }

  if (opts.command === 'sync') { await syncMain(branch, opts.retries); return; }

  if (branch === 'main' || branch === 'master') {
    throw new Error('当前在 ' + branch + ' 上：本项目禁止直推 main（PRD CODE-09）。请先 git switch -c <分支>。');
  }

  if (opts.command === 'run' || opts.command === 'open') {
    console.log('  推送分支…');
    await gitNet(['push', '-u', 'origin', 'HEAD'], { retries: opts.retries, label: '推送' });
    const pr = await ensureOpenPr(opts, slug, branch, headSha);
    if (opts.command === 'open') { console.log('  ✅ 分支与 PR 就绪（未等待检查）'); return; }
  }

  const activePr = prNumber ?? (await readState()).pr;
  if (!activePr) throw new Error('没有可用的 PR：请先执行 open，或在分支上重跑 run');
  const prInfo = await api('/repos/' + slug + '/pulls/' + activePr);
  const prSha = prInfo.json.head.sha;

  if (opts.merge) {
    console.log('  等待必需检查（上限 ' + opts.wait + ' 分钟）…');
    const verdict = await waitForChecks(opts, slug, activePr, prSha);
    if (!verdict.ok) {
      console.log('  ❌ ' + verdict.reason);
      console.log('  未合并。修好后提交并重跑：node scripts/pr-flow.mjs run');
      process.exitCode = 1;
      return;
    }
    const sha = await mergePr(opts, slug, activePr);
    await writeState({ mergedSha: sha, pr: undefined });
  } else {
    console.log('  （--no-merge：跳过合并）');
  }

  await syncMain(branch, opts.retries);
  if (opts.verifyMirror) await verifyMirror(slug);
  console.log('  🎉 完成：' + git(['log', '-1', '--pretty=%h %s']));
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  main().catch((err) => {
    console.error('❌ ' + (err && err.message ? err.message : String(err)));
    process.exitCode = 1;
  });
}
