#!/usr/bin/env node
// scripts/publish-gitee-release.mjs
// 替代已停更的 semantic-release-gitee（last publish 2022-02，违反红线 R-16）。
// 直接调用 Gitee OpenAPI v5 完成「创建/复用 Release + 上传附件」。
//
// 2026-10-13 拆分：CLI 解析 / glob 展开 / HTTP 出口 / 发布域逻辑 分到 scripts/lib/，
// 本文件只保留 CLI 契约与编排（CODE-11：文件 SLOC ≤ 200、函数 ≤ 100）。
// 拆分是行为等价重构，等价性由 --dry-run 输出逐字节 diff 证明；并新增 --self-test。
//
// 用法：
//   node scripts/publish-gitee-release.mjs --tag v1.0.0 --notes-file CHANGELOG.md --assets "dist/**/*"
//       环境变量：GITEE_TOKEN（必填）、GITEE_OWNER、GITEE_REPO
//       可选：--target <sha|branch>、--prerelease、--dry-run、--timeout <ms>、--verify、--self-test
//
// 设计要点：
//   1. 零第三方依赖（Node >= 18 的 fetch/FormData/Blob）。
//   2. --dry-run 打印全部请求（含 body 预览），用于首次联调与 CI 冒烟。
//   3. 幂等：按 tag 查询已有 Release，存在则复用（不重复创建）。
//   4. 附件重名冲突视为「已存在」跳过，其余错误立即失败（fail fast）。
//   5. 不做任何静默降级：失败即非零退出，并打印 Gitee 返回体便于定位。
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import process from "node:process";

import { parseArgs, runSelfTest as runArgsSelfTest } from "./lib/release-args.mjs";
import { expandAssets, runSelfTest as runGlobSelfTest } from "./lib/release-glob.mjs";
import { fail, hasReported } from "./lib/release-http.mjs";
import { createRelease, findOrReuseRelease, uploadAssets, verifyEndpoint } from "./lib/gitee-release.mjs";

// --self-test 必须在 parseArgs 之前处理：它不是 CLI 参数，只是脚本自检入口。
if (process.argv.includes("--self-test")) {
  const a = runArgsSelfTest();
  const b = await runGlobSelfTest();
  console.log("release-args 自检：" + a.pass + "/" + a.total + " 通过");
  console.log("release-glob 自检：" + b.pass + "/" + b.total + " 通过");
  const pass = a.pass + b.pass;
  const total = a.total + b.total;
  console.log("合计：" + pass + "/" + total + " 通过");
  process.exit(pass === total ? 0 : 1);
}

/** 校验入参与环境变量，并读出 notes / name（原样保留校验顺序与文案）。 */
async function resolveContext(args) {
  const token = process.env.GITEE_TOKEN;
  const owner = args.owner || process.env.GITEE_OWNER;
  const repo = args.repo || process.env.GITEE_REPO;
  if (!args.tag) fail("缺少 --tag");
  if (!args.dryRun && !args.verify && !token) fail("缺少环境变量 GITEE_TOKEN");
  if (!owner || !repo) fail("缺少 owner/repo（--owner/--repo 或 GITEE_OWNER/GITEE_REPO）");
  const notes = args.notesFile ? await readFile(args.notesFile, "utf8") : (args.notes || "");
  const name = args.name || args.tag;
  return { args, token, owner, repo, notes, name };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  // 2026-10-13 修复：此前缺少 utf8，--help 打印的是 Buffer 对象而不是文本。
  if (args.help) { console.log(await readFile(new URL(import.meta.url), "utf8")); return; }
  const ctx = await resolveContext(args);
  if (args.verify) return verifyEndpoint(ctx);

  const files = await expandAssets(args.assets, resolve("."));
  const suffix = args.dryRun ? "  [DRY-RUN]" : "";
  console.log("发布目标：" + ctx.owner + "/" + ctx.repo + "  tag=" + args.tag + "  资产 " + files.length + " 个" + suffix);

  const found = await findOrReuseRelease(ctx);
  const releaseId = found.existed ? found.releaseId : await createRelease(ctx);
  const { uploaded, skipped } = await uploadAssets(ctx, releaseId, files);

  const prefix = args.dryRun ? "[DRY-RUN] " : "";
  console.log("完成：" + prefix + "新建资产 " + uploaded + " 个，跳过 " + skipped + " 个");
}

main().catch((e) => {
  if (!hasReported()) console.error("❌ " + (e && e.message ? e.message : String(e)));
  process.exitCode = 1;
});
