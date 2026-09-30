#!/usr/bin/env bash
# 前端路径封装检查：确保「路径校验只在 Rust 侧」这条红线未被破坏。
# ESLint 的 no-restricted-syntax 只能覆盖 AST 可识别的调用形式，
# 本脚本补充扫描字符串字面量中的危险模式。
set -euo pipefail

# ── 依赖守卫（必须放在最前面）──────────────────────
# ⚠️ ripgrep 并非 runner 预装。若缺失时不显式报错，下方
#    `if hits=$(rg ...)` 会因退出码 127 被判定为「无命中」，
#    导致门禁静默通过、形同虚设。故此处硬性失败。
if ! command -v rg >/dev/null 2>&1; then
  echo "❌ 缺少依赖：ripgrep (rg)。请先安装（CI 见 §11.4 的安装步骤）："
  echo "   Ubuntu/Debian: sudo apt-get install -y ripgrep"
  echo "   macOS:         brew install ripgrep"
  echo "   Windows:       winget install BurntSushi.ripgrep.MSVC"
  exit 2
fi

violations=0

# 只扫描前端源码，Rust 侧路径操作是合法的
SCAN_PATHS=(src)
EXCLUDES=(--glob '!**/*.d.ts' --glob '!src/core/ipc/**')

# rg 封装：退出码 0=命中，1=无命中，≥2=执行错误（必须显式失败，不得视为"通过"）
rg_scan() {
  local out status
  out=$(rg -n "${EXCLUDES[@]}" "$@" "${SCAN_PATHS[@]}" 2>&1) && status=0 || status=$?
  if [[ $status -ge 2 ]]; then
    echo "❌ ripgrep 执行失败（exit $status），门禁不可判定："
    echo "$out" | head -5 | sed 's/^/   /'
    exit 2
  fi
  printf '%s' "$out"
}

check() {
  local pattern="$1" reason="$2"
  local hits
  hits=$(rg_scan --regexp "$pattern")
  if [[ -n "$hits" ]]; then
    echo "❌ 路径封装违规：$reason"
    echo "$hits" | sed 's/^/   /'
    violations=$((violations + 1))
  fi
}

# 1. 绝对路径字面量：前端不应出现盘符或根路径开头的字符串
check "\"[A-Za-z]:[\\\\/]"            '前端出现 Windows 绝对路径字面量（应由 Rust 侧解析）'
check "'[A-Za-z]:[\\\\/]"             '前端出现 Windows 绝对路径字面量（应由 Rust 侧解析）'
check '"/(Users|home|root)/'          '前端出现 Unix 绝对路径字面量（应由 Rust 侧解析）'

# 2. 路径回溯片段：前端不得自行拼接 ../ 或 ..\
#    ⚠️ 必须排除模块导入语句——TS/ESM 的相对导入（from '../lib/ipc'）
#    是合法且普遍写法，若不排除会导致门禁全量误报。
#    实现方式：先用 rg 粗筛出含 ../ 的行，再用 grep -v 剔除导入语句。
#    不用 `rg -P`（PCRE2）：部分发行版打包的 ripgrep 未启用该特性，会直接报错。
traversal_hits=$(rg_scan --regexp '\.\.[\\/]' \
  | grep -Ev '^[^:]+:[0-9]+:\s*(import|export)\b' \
  | grep -Ev '\bfrom\s+['"'"'"]' \
  | grep -Ev '\brequire\s*\(' \
  || true)
if [[ -n "$traversal_hits" ]]; then
  echo "❌ 路径封装违规：前端出现路径回溯片段（路径穿越风险，SEC-02）"
  echo "   （模块导入语句已排除，以下均为可疑的运行时路径拼接）"
  echo "$traversal_hits" | sed 's/^/   /'
  violations=$((violations + 1))
fi

# 3. 直接触碰 Tauri fs 插件（本项目明确不引入 tauri-plugin-fs，PRD AC-01）
check '@tauri-apps/plugin-fs'         '禁止引入 tauri-plugin-fs，文件操作必须走自定义 Command'

# 4. 绕过 IPC 封装直接调用 invoke
hits=$(rg_scan --regexp 'invoke\s*[<(]')
if [[ -n "$hits" ]]; then
  echo "❌ IPC 封装违规：禁止在 src/core/ipc/ 之外直接调用 invoke()"
  echo "$hits" | sed 's/^/   /'
  violations=$((violations + 1))
fi

if [[ $violations -gt 0 ]]; then
  echo ""
  echo "路径封装检查失败，共 $violations 类违规。详见技术方案 §6.2 与 §7.3。"
  exit 1
fi

echo "✅ 路径封装检查通过"
