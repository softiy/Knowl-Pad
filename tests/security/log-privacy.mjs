#!/usr/bin/env node
// 门禁 11 子项：日志隐私静态检查（技术方案 §9.5「CI 门禁」；PRD SEC-09 / T-12 / ERR-04）。
//
// 规则：tracing 宏的**参数**中不得出现可能承载用户内容的标识符
// （笔记正文、frontmatter、搜索词、标签名等）。字符串字面量先被剥离，避免误报。
// 例外：行内或上一行含 "kp-log-ok:" 注释时跳过（需在注释中说明理由）。
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import process from "node:process";

const ROOT = process.cwd();
const SCAN_DIRS = [join(ROOT, "src-tauri", "src"), join(ROOT, "crates")];
const MACRO = /tracing::(trace|debug|info|warn|error)!\s*\(/g;
// §9.5 表格「禁止记录」一栏对应的标识符
const FORBIDDEN = /(?<![A-Za-z0-9_])(content|text|body|frontmatter|query|keyword|search_query|plain_text|note_body|raw_text|file_content|tag|tags)(?![A-Za-z0-9_])/;

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) walk(full, out);
    else if (name.endsWith(".rs")) out.push(full);
  }
  return out;
}

/** 从宏名后的 "(" 起，取到配对括号结束的参数文本。 */
function macroArgs(source, openParen) {
  let depth = 0;
  for (let i = openParen; i < source.length; i += 1) {
    const ch = source[i];
    if (ch === "(") depth += 1;
    else if (ch === ")") {
      depth -= 1;
      if (depth === 0) return source.slice(openParen + 1, i);
    }
  }
  return source.slice(openParen + 1);
}

/** 移除字符串字面量（含转义），只保留标识符与标点。 */
function stripStrings(text) {
  return text.replace(/"(?:\\.|[^"\\])*"/g, '""');
}

/** 扫描一段源码，返回违规点（供主流程与 --self-test 共用）。 */
export function scanSource(source, file = "<inline>") {
  const lines = source.split("\n");
  const found = [];
  let checked = 0;
  MACRO.lastIndex = 0;
  let match;
  while ((match = MACRO.exec(source)) !== null) {
    const openParen = source.indexOf("(", match.index);
    const args = stripStrings(macroArgs(source, openParen));
    checked += 1;
    const hit = FORBIDDEN.exec(args);
    if (!hit) continue;
    const lineNumber = source.slice(0, match.index).split("\n").length;
    const context = `${lines[lineNumber - 2] ?? ""}\n${lines[lineNumber - 1] ?? ""}`;
    if (context.includes("kp-log-ok:")) continue;
    found.push({ file, line: lineNumber, token: hit[1] });
  }
  return { found, checked };
}

// 自检模式：证明本门禁**确实能抓到违规**（避免门禁本身空转）
if (process.argv.includes("--self-test")) {
  const dirty = [
    "tracing::info!(content = %note.content, \"读取\");",
    "tracing::warn!(%text, \"解析失败\");",
    "tracing::debug!(query = %q, \"搜索\");",
  ].join("\n");
  const clean = [
    "tracing::info!(code = err.code(), rel_path = rel, \"拒绝\");",
    "// kp-log-ok: 已确认不含用户内容",
    "tracing::info!(text = %raw, \"允许的例外\");",
  ].join("\n");
  const dirtyResult = scanSource(dirty);
  const cleanResult = scanSource(clean);
  const ok = dirtyResult.found.length === 3 && cleanResult.found.length === 0;
  console.log(`自检：违规样本命中 ${dirtyResult.found.length}/3，干净样本误报 ${cleanResult.found.length}/0`);
  if (!ok) {
    console.error("❌ 日志隐私门禁自检失败：检测逻辑失效");
    process.exit(1);
  }
  console.log("✅ 日志隐私门禁自检通过");
  process.exit(0);
}

const files = SCAN_DIRS.flatMap((dir) => {
  try {
    return walk(dir);
  } catch {
    return [];
  }
});

const violations = [];
let macrosChecked = 0;

for (const file of files) {
  const { found, checked } = scanSource(readFileSync(file, "utf8"), relative(ROOT, file));
  macrosChecked += checked;
  violations.push(...found);
}

console.log(`▶ 日志隐私检查：扫描 ${files.length} 个 Rust 文件，检查 ${macrosChecked} 处 tracing 调用`);
if (violations.length > 0) {
  for (const v of violations) {
    console.error(`❌ ${v.file}:${v.line} 疑似记录用户内容（字段 "${v.token}"）——见技术方案 §9.5`);
  }
  console.error(`\n❌ 日志隐私检查失败：${violations.length} 处`);
  process.exit(1);
}
console.log("✅ 日志隐私检查通过");
