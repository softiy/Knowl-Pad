#!/usr/bin/env node
// CODE-11：Rust 单文件行数限制（警告 200 / 硬性上限 350）。
//
// PRD CODE-11 原文：超警告阈值报 warning；超硬性上限 CI 报 error 并拒绝合并；
// 「测试文件与 schema.rs（DDL 定义）豁免硬性上限」。
//
// 豁免规则（本脚本的口径）：
//   - 路径含 /tests/ 或文件名以 _tests.rs / _test.rs 结尾（测试文件）
//   - 文件名以 schema.rs 结尾（DDL 定义，如 index_schema.rs）
// 用法：node scripts/check-file-length.mjs [--self-test]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import process from "node:process";

const ROOT = process.cwd();
const SCAN_DIRS = [join(ROOT, "src-tauri", "src"), join(ROOT, "crates")];
const WARN_LIMIT = 200;
const HARD_LIMIT = 350;

export function isExempt(relPath) {
  const normalized = relPath.replace(/\\/g, "/");
  return (
    normalized.includes("/tests/") ||
    /_tests?\.rs$/.test(normalized) ||
    /schema\.rs$/.test(normalized)
  );
}

export function measure(relPath, content) {
  // 物理行数（与 wc -l 同口径）：末尾换行不计为额外一行
  const newlines = (content.match(/\n/g) ?? []).length;
  const lines = content.endsWith("\n") ? newlines : newlines + 1;
  const exempt = isExempt(relPath);
  let level = "ok";
  if (lines > HARD_LIMIT && !exempt) level = "error";
  else if (lines > WARN_LIMIT && !exempt) level = "warn";
  return { lines, exempt, level };
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) walk(full, out);
    else if (name.endsWith(".rs")) out.push(full);
  }
  return out;
}

if (process.argv.includes("--self-test")) {
  const cases = [
    ["src-tauri/src/a.rs", 351, "error"],
    ["src-tauri/src/a.rs", 201, "warn"],
    ["src-tauri/src/a.rs", 200, "ok"],
    ["crates/kp-domain/tests/big.rs", 900, "ok"],
    ["src-tauri/src/storage/index_schema.rs", 900, "ok"],
    ["src-tauri/src/commands/vault_tests.rs", 900, "ok"],
  ];
  let pass = 0;
  for (const [path, lines, want] of cases) {
    const content = Array.from({ length: lines }, () => "x").join("\n") + "\n";
    const got = measure(path, content).level;
    if (got === want) pass += 1;
    else console.error(`❌ 自检失败：${path} ${lines} 行 期望 ${want} 实得 ${got}`);
  }
  console.log(`自检：${pass}/${cases.length} 通过`);
  process.exit(pass === cases.length ? 0 : 1);
}

const files = SCAN_DIRS.flatMap((dir) => { try { return walk(dir); } catch { return []; } });
const results = files.map((f) => {
  const rel = relative(ROOT, f);
  return { rel, ...measure(rel, readFileSync(f, "utf8")) };
});
const errors = results.filter((r) => r.level === "error");
const warns = results.filter((r) => r.level === "warn");

console.log(`▶ CODE-11 文件长度检查：${results.length} 个 Rust 文件（豁免 ${results.filter((r) => r.exempt).length} 个）`);
for (const w of warns) console.warn(`⚠️  ${w.rel} 为 ${w.lines} 行（警告阈值 ${WARN_LIMIT}）`);
for (const e of errors) console.error(`❌ ${e.rel} 为 ${e.lines} 行，超过硬性上限 ${HARD_LIMIT}`);
if (errors.length > 0) {
  console.error(`\n❌ CODE-11 失败：${errors.length} 个文件超限`);
  process.exit(1);
}
console.log(`✅ CODE-11 通过（超硬性上限 0 个，超警告阈值 ${warns.length} 个）`);
