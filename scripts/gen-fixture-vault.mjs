#!/usr/bin/env node
// 基准数据集生成器（PRD §6.1.1 的三档：标准库 / 大库 / 压力库）。确定性伪随机（固定 seed）。
//
// 2026-10-13（M5 批次「性能设施」）：
//   ① 补上 PRD §6.1.1 明确要求的 **15% 附件**（此前只生成 .md，验收口径与需求不符）；
//   ② 支持 --count/--kb 覆盖（CI 用得起的小规模、真机验收用大规模，不必改代码）；
//   ③ 结束时打印**清单**（文件数/目录数/字节数 + 附件数），供 D-25 的采样脚本核对"夹具真的是这一档"。
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";

const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const tier = arg("--tier", "standard");
const out = arg("--out", join(process.env.TEMP ?? "/tmp", "kp-vault-" + tier));
// PRD §6.1.1：标准库 3000 篇 × 2KB（含 15% 附件）；大库 20000 篇 × 4KB；压力库 100000 篇。
const PRESET = { standard: { count: 3000, kb: 2 }, large: { count: 20000, kb: 4 }, stress: { count: 100000, kb: 2 } };
const preset = PRESET[tier] ?? PRESET.standard;
const count = Number(arg("--count", preset.count));
const kb = Number(arg("--kb", preset.kb));
const attachRatio = Number(arg("--attach-ratio", 0.15));
let seed = 42;
const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff; };

console.log("生成 " + tier + " 夹具：" + count + " 篇 × ~" + kb + "KB，附件比例 " + attachRatio + " → " + out);
await mkdir(out, { recursive: true });
let dirs = 0;
let bytes = 0;
let attaches = 0;
const dirCount = Math.max(1, Math.min(50, Math.ceil(count / 60)));
for (let i = 0; i < count; i++) {
  const d = join(out, "d" + (i % dirCount));
  await mkdir(d, { recursive: true });
  const lines = ["---", "tags: [a, b/c]", "aliases: [x" + i + "]", "---", "", "# 笔记 " + i, ""];
  while (Buffer.byteLength(lines.join("\n"), "utf8") < kb * 1024) {
    lines.push("内容 " + Math.floor(rnd() * 1e6) + " 与 [[笔记 " + Math.floor(rnd() * count) + "]] 以及 #标签/" + (i % 20));
  }
  const body = lines.join("\n");
  await writeFile(join(d, "note-" + i + ".md"), body, "utf8");
  bytes += Buffer.byteLength(body, "utf8");
  // 15% 附件：非 .md 的二进制旁挂文件（PRD §6.1.1 的"含 15% 附件"）
  if (rnd() < attachRatio) {
    const ab = Buffer.alloc(1024 + Math.floor(rnd() * 4096), i % 251);
    await writeFile(join(d, "attach-" + i + ".png"), ab);
    attaches += 1;
    bytes += ab.length;
  }
  if (i % 5000 === 0 && i > 0) console.log("  " + i + "/" + count);
}
dirs = dirCount;
const manifest = { tier, count, kb, attachRatio, notes: count, attachments: attaches, dirs, bytes };
console.log("✅ 夹具生成完成：" + JSON.stringify(manifest));
await writeFile(join(out, ".kp-fixture-manifest.json"), JSON.stringify(manifest, null, 2) + "\n", "utf8");
