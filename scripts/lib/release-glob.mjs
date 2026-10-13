// 极简 glob 与资产展开（抽取自 publish-gitee-release.mjs）。零依赖、无网络；自带 --self-test。
import { readdir, mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import process from "node:process";

/** 极简 glob：支持 **、*、?，路径统一为 / 分隔。不引入任何依赖。 */
export function globToRegExp(pattern) {
  const norm = pattern.replace(/\\/g, "/");
  let re = "";
  for (let i = 0; i < norm.length; i++) {
    const c = norm[i];
    if (c === "*") {
      if (norm[i + 1] === "*") {
        // ** 匹配任意层级（含 /）
        i++;
        if (norm[i + 1] === "/") i++;
        re += "(?:.*/)?";
      } else {
        re += "[^/]*";
      }
    } else if (c === "?") {
      re += "[^/]";
    } else if ("\\^$.|+()[]{}".includes(c)) {
      re += "\\" + c;
    } else {
      re += c;
    }
  }
  return new RegExp("^" + re + "$");
}

export async function expandAssets(patterns, root) {
  if (!patterns.length) return [];
  const regexes = patterns.map(globToRegExp);
  const found = [];
  async function walk(dir) {
    let entries;
    try { entries = await readdir(dir, { withFileTypes: true }); }
    catch { return; }
    for (const e of entries) {
      const p = dir + "/" + e.name;
      const rel = p.slice(root.length + 1);
      if (e.isDirectory()) await walk(p);
      else if (regexes.some((r) => r.test(rel))) found.push(p);
    }
  }
  await walk(root);
  return [...new Set(found)].sort();
}

export async function runSelfTest() {
  const cases = [
    ["** 跨层级匹配", globToRegExp("dist/**/*").test("dist/a/b/c.exe")],
    ["** 也匹配零层（dist/**/* 命中 dist/x.exe）", globToRegExp("dist/**/*").test("dist/x.exe")],
    ["dist/**/* 覆盖该目录下全部文件（含非 exe）", globToRegExp("dist/**/*").test("dist/note.txt")],
    ["* 不跨 /", globToRegExp("a/*.exe").test("a/b/c.exe") === false],
    ["* 匹配同级", globToRegExp("a/*.exe").test("a/x.exe")],
    ["? 匹配单字符且不跨 /", globToRegExp("a?.md").test("ab.md") && globToRegExp("a?.md").test("a/b.md") === false],
    ["特殊字符按字面量", globToRegExp("v1.2.3.md").test("v1.2.3.md") && globToRegExp("v1.2.3.md").test("v1x2x3.md") === false],
    ["反斜杠归一化", globToRegExp("dist\\**\\*.exe").test("dist/a/b.exe")],
    ["中文文件名", globToRegExp("docs/*.md").test("docs/中文说明.md")],
  ];
  let pass = 0;
  for (const [name, ok] of cases) { if (ok) pass += 1; else console.error("❌ 自检失败（globToRegExp）：" + name); }
  const total0 = cases.length;
  // expandAssets：去重 + 排序 + 只收匹配项（用临时目录，不碰仓库）
  const dir = await mkdtemp(join(tmpdir(), "kp-assets-"));
  try {
    await mkdir(join(dir, "dist", "sub"), { recursive: true });
    await writeFile(join(dir, "dist", "b.exe"), "b");
    await writeFile(join(dir, "dist", "a.exe"), "a");
    await writeFile(join(dir, "dist", "sub", "c.exe"), "c");
    await writeFile(join(dir, "dist", "note.txt"), "n");
    const got = await expandAssets(["dist/**/*"], dir);
    const names = got.map((p) => p.slice(dir.length + 1));
    const expect = ["dist/a.exe", "dist/b.exe", "dist/note.txt", "dist/sub/c.exe"];
    const sorted = JSON.stringify(names) === JSON.stringify(expect);
    if (sorted) pass += 1; else console.error("❌ 自检失败（expandAssets 排序/过滤）：" + JSON.stringify(names));
    const dup = await expandAssets(["dist/*.exe", "dist/**/*"], dir);
    const dupNames = dup.map((p) => p.slice(dir.length + 1));
    const dedup = JSON.stringify(dupNames) === JSON.stringify(expect);
    if (dedup) pass += 1; else console.error("❌ 自检失败（expandAssets 去重）：" + JSON.stringify(dupNames));
    const none = await expandAssets([], dir);
    if (none.length === 0) pass += 1; else console.error("❌ 自检失败（空 patterns 应为空）");
    const missing = await expandAssets(["nope/**/*"], dir);
    if (missing.length === 0) pass += 1; else console.error("❌ 自检失败（不存在的目录应返回空而非抛错）");
  } finally { await rm(dir, { recursive: true, force: true }); }
  return { pass, total: total0 + 4 };
}

const RUN_AS_SCRIPT = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (RUN_AS_SCRIPT) {
  const r = await runSelfTest();
  console.log("release-glob 自检：" + r.pass + "/" + r.total + " 通过");
  process.exit(r.pass === r.total ? 0 : 1);
}
