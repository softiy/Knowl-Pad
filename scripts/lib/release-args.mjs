// Gitee 发布脚本的 CLI 解析（抽取自 publish-gitee-release.mjs：CODE-11 文件/函数级上限）。
// 纯函数、零依赖；自带 --self-test（脚本此前完全没有测试网）。
import { pathToFileURL } from "node:url";
import process from "node:process";

export function parseArgs(argv) {
  const out = { prerelease: false, dryRun: false, timeout: 60000, assets: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error("缺少 " + a + " 的值");
      return v;
    };
    switch (a) {
      case "--tag": out.tag = next(); break;
      case "--name": out.name = next(); break;
      case "--notes-file": out.notesFile = next(); break;
      case "--notes": out.notes = next(); break;
      case "--assets": out.assets.push(next()); break;
      case "--target": out.target = next(); break;
      case "--owner": out.owner = next(); break;
      case "--repo": out.repo = next(); break;
      case "--timeout": out.timeout = Number(next()); break;
      case "--prerelease": out.prerelease = true; break;
      case "--dry-run": out.dryRun = true; break;
      case "--verify": out.verify = true; break;
      case "-h": case "--help": out.help = true; break;
      default: throw new Error("未知参数：" + a);
    }
  }
  return out;
}

export function runSelfTest() {
  const cases = [
    ["默认值（超时 60s、assets 空、三个开关为 false）", (() => { const a = parseArgs([]); return a.timeout === 60000 && a.assets.length === 0 && a.prerelease === false && a.dryRun === false; })()],
    ["--tag 取值", parseArgs(["--tag", "v1.2.3"]).tag === "v1.2.3"],
    ["--assets 可多次出现且保序", (() => { const a = parseArgs(["--assets", "a/*", "--assets", "b/*"]); return a.assets.length === 2 && a.assets[0] === "a/*" && a.assets[1] === "b/*"; })()],
    ["--timeout 转数字", parseArgs(["--timeout", "1234"]).timeout === 1234],
    ["--prerelease/--dry-run/--verify 为布尔", (() => { const a = parseArgs(["--prerelease", "--dry-run", "--verify"]); return a.prerelease === true && a.dryRun === true && a.verify === true; })()],
    ["-h 与 --help 等价", parseArgs(["-h"]).help === true && parseArgs(["--help"]).help === true],
    ["缺少值时抛错", (() => { try { parseArgs(["--tag"]); return false; } catch (e) { return /缺少 --tag 的值/.test(e.message); } })()],
    ["未知参数抛错", (() => { try { parseArgs(["--nope"]); return false; } catch (e) { return /未知参数/.test(e.message); } })()],
  ];
  let pass = 0;
  for (const [name, ok] of cases) { if (ok) pass += 1; else console.error("❌ 自检失败（parseArgs）：" + name); }
  return { pass, total: cases.length };
}

const RUN_AS_SCRIPT = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (RUN_AS_SCRIPT) {
  const r = runSelfTest();
  console.log("release-args 自检：" + r.pass + "/" + r.total + " 通过");
  process.exit(r.pass === r.total ? 0 : 1);
}
