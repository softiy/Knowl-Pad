// Gitee Release 的发布域逻辑（抽取自 publish-gitee-release.mjs 的 main()）。
// 四个步骤函数对应原文的四个分支，日志文案逐字保留 —— 拆分是行为等价重构，
// 等价性由 --dry-run 输出的逐字节 diff 证明（见 PR 说明）。
import { readFile, stat } from "node:fs/promises";
import { basename } from "node:path";
import { API, fail, request } from "./release-http.mjs";

/** --verify：只读联调（退出码 0 = 已存在、2 = 尚无 Release 属正常、1 = 失败）。 */
export async function verifyEndpoint(ctx) {
  const { args, token, owner, repo } = ctx;
  const q = token ? "?access_token=" + encodeURIComponent(token) : "";
  const url = API + "/repos/" + owner + "/" + repo + "/releases/tags/" + encodeURIComponent(args.tag) + q;
  const res = await request(url, { method: "GET" }, args.timeout, "查询 Release");
  if (res.status === 200) {
    const rel = JSON.parse(res.text);
    if (!rel || typeof rel !== "object") {
      console.log("✅ 端点可达 · 该 tag 尚无 Release（Gitee 对不存在的 tag 返回 200 + null，属正常）");
      process.exitCode = 2;
      return;
    }
    console.log("✅ 端点可达 · Release 已存在：id=" + rel.id + " tag=" + rel.tag_name + " name=" + (rel.name || ""));
    const attUrl = API + "/repos/" + owner + "/" + repo + "/releases/" + rel.id + "/attach_files?per_page=100" + (token ? "&access_token=" + encodeURIComponent(token) : "");
    const att = await request(attUrl, { method: "GET" }, args.timeout, "查询附件");
    if (att.status === 200) {
      const list = JSON.parse(att.text);
      const names = list.length ? "：" + list.map((a) => a.name).join(", ") : "";
      console.log("   已上传附件 " + list.length + " 个" + names);
    }
    process.exitCode = 0;
    return;
  }
  if (res.status === 404) {
    console.log("✅ 端点可达 · 该 tag 尚无 Release（首次发布属正常）");
    process.exitCode = 2;
    return;
  }
  if (res.status === 401 || res.status === 403) {
    console.error("❌ token 无效或权限不足（HTTP " + res.status + "）；Gitee 私人令牌需勾选 projects 权限");
    process.exitCode = 1;
    return;
  }
  console.error("❌ 意外响应 HTTP " + res.status, res.text);
  process.exitCode = 1;
}

/** 幂等：按 tag 查询已有 Release，存在则复用（Gitee 对不存在的 tag 返回 200 + null）。 */
export async function findOrReuseRelease(ctx) {
  const { args, token, owner, repo } = ctx;
  const getUrl = API + "/repos/" + owner + "/" + repo + "/releases/tags/" + encodeURIComponent(args.tag) + "?access_token=" + (token || "");
  if (args.dryRun) console.log("→ GET " + getUrl.replace(token || "x", "<TOKEN>"));
  let releaseId = null;
  let existed = false;
  if (!args.dryRun) {
    const r = await request(getUrl, { method: "GET" }, args.timeout, "查询 Release");
    if (r.status === 200) {
      const rel = JSON.parse(r.text);
      if (rel && typeof rel === "object") {
        releaseId = rel.id;
        existed = true;
        console.log("✓ 已存在 Release id=" + releaseId + "，复用");
      } else {
        console.log("（Gitee 对不存在的 tag 返回 200 + null，按尚未创建处理）");
      }
    } else if (r.status !== 404) {
      fail("查询 Release 失败（HTTP " + r.status + "）", r.text);
    }
  }
  return { releaseId, existed };
}

/** 创建 Release（dry-run 只打印请求体）。返回新的 releaseId（dry-run 下为 null）。 */
export async function createRelease(ctx) {
  const { args, token, owner, repo, notes, name } = ctx;
  const form = new URLSearchParams({
    access_token: token || "<TOKEN>",
    tag_name: args.tag,
    name: name,
    body: notes,
  });
  if (args.target) form.set("target_commitish", args.target);
  if (args.prerelease) form.set("prerelease", "true");
  const createUrl = API + "/repos/" + owner + "/" + repo + "/releases";
  if (args.dryRun) {
    console.log("→ POST " + createUrl);
    const shown = form.toString().replace(encodeURIComponent("<TOKEN>"), "<TOKEN>").replace(token || "x", "<TOKEN>");
    console.log("  form: " + shown);
    return null;
  }
  const r = await request(createUrl, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: form.toString(),
  }, args.timeout, "创建 Release");
  if (r.status !== 201 && r.status !== 200) fail("创建 Release 失败（HTTP " + r.status + "）", r.text);
  const id = JSON.parse(r.text).id;
  console.log("✓ Release 创建成功 id=" + id);
  return id;
}

/** 逐个上传附件：重名视为已存在跳过，其余错误立即失败。返回计数。 */
export async function uploadAssets(ctx, releaseId, files) {
  const { args, token, owner, repo } = ctx;
  let uploaded = 0;
  let skipped = 0;
  for (const f of files) {
    const size = (await stat(f)).size;
    const uploadUrl = API + "/repos/" + owner + "/" + repo + "/releases/" + (releaseId ?? "<ID>") + "/attach_files";
    if (args.dryRun) {
      console.log("→ POST " + uploadUrl + "  file=" + f + " (" + size + " bytes)");
      uploaded++;
      continue;
    }
    const buf = await readFile(f);
    const fd = new FormData();
    fd.append("access_token", token);
    fd.append("file", new Blob([buf]), basename(f));
    const r = await request(uploadUrl, { method: "POST", body: fd }, Math.max(args.timeout, 120000), "上传 " + basename(f));
    if (r.status === 201 || r.status === 200) {
      uploaded++;
      console.log("  ✓ " + basename(f) + " (" + size + " bytes)");
    } else if (/exist|已存在|already/i.test(r.text)) {
      skipped++;
      console.log("  ↷ " + basename(f) + " 已存在，跳过");
    } else {
      fail("上传 " + basename(f) + " 失败（HTTP " + r.status + "）", r.text);
    }
  }
  return { uploaded, skipped };
}
