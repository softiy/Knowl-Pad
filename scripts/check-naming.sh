#!/usr/bin/env bash
# 命名一致性检查：扫描 PRD §0.2 定义的禁止写法。
# 退出码非 0 即阻断 CI。
set -euo pipefail

# ── 依赖守卫（必须放在最前面）──────────────────────
# ⚠️ ripgrep 并非 runner 预装：GitHub Actions 的 ubuntu-latest 不含 rg，
#    Windows/macOS 开发机通常也不含。若缺失时不显式报错，下方的
#    `if hits=$(rg ...)` 会因退出码 127 被判定为「无命中」，
#    导致门禁静默通过、形同虚设。故此处硬性失败。
if ! command -v rg >/dev/null 2>&1; then
  echo "❌ 缺少依赖：ripgrep (rg)。请先安装（CI 见 §11.4 的安装步骤）："
  echo "   Ubuntu/Debian: sudo apt-get install -y ripgrep"
  echo "   macOS:         brew install ripgrep"
  echo "   Windows:       winget install BurntSushi.ripgrep.MSVC"
  exit 2
fi

# 扫描范围：源码与文档，排除产物目录、依赖、以及规范说明自身所在文件。
# ⚠️ 只纳入**实际存在**的目录：ripgrep 对不存在的路径会以退出码 2 失败，
#    而 `if hits=$(rg ...)` 会把任何非零退出码当成「无命中」，导致门禁静默通过。
SCAN_PATHS=()
for p in src src-tauri/src tests scripts docs; do
  [[ -d "$p" ]] && SCAN_PATHS+=("$p")
done
if [[ ${#SCAN_PATHS[@]} -eq 0 ]]; then
  echo "❌ 未找到任何待扫描目录，门禁不可判定"; exit 2
fi
EXCLUDES=(
  --glob '!**/target/**'
  --glob '!**/node_modules/**'
  --glob '!**/dist/**'
  --glob '!**/dist-*/**'
  # ⚠️ 以下三个文件必须排除：它们为定义/执行规范而必须列举禁止写法，
  #    否则脚本会扫描到自身内容并必然失败（自指悖论）
  #    用 `!**/<basename>` 而非 `!docs/<basename>`：rg 的 --glob 匹配语义
  #    依赖调用时的根路径，按 basename 排除更稳健。
  --glob '!**/Knowl-Pad-PRD.md'              # §0.2 命名规范定义处
  --glob '!**/Knowl-Pad-完整技术方案.md'      # §11.7.8 内嵌了本脚本源码
  --glob '!**/AGENTS.md'                     # 红线摘要同样列举了禁止写法（自指）
  --glob '!**/check-naming.sh'               # 本脚本自身
)

violations=0

# rg 封装：退出码 0=命中，1=无命中，≥2=执行错误。
# ⚠️ 执行错误**必须**显式失败，绝不能当成"无命中"（否则门禁静默通过、形同虚设）。
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
  # -n 输出行号便于定位；--fixed-strings 避免正则元字符误判
  hits=$(rg_scan --fixed-strings "$pattern")
  if [[ -n "$hits" ]]; then
    echo "❌ 命名违规：发现 '$pattern'（$reason）"
    echo "$hits" | sed 's/^/   /'
    violations=$((violations + 1))
  fi
}

# ── 产品名禁止写法 ────────────────────────────────
check 'KnowlPad'   '驼峰无空格；仅允许出现在安装包产物文件名中'
check 'knowlPad'   '小驼峰'
check 'Knowl-pad'  '产品名语境下的错误大小写'

# ── 标识符禁止写法 ────────────────────────────────
# 注意：Rust 模块名 knowl_pad:: 是语言强制约定，属合法例外，故此处
# 仅检查「作为包名/crate 名字符串」出现的下划线写法
hits=$(rg_scan --regexp '"name"\s*[:=]\s*"knowl_pad"' --regexp '^name\s*=\s*"knowl_pad"')
if [[ -n "$hits" ]]; then
  echo "❌ 命名违规：包名/crate 名应为 'knowl-pad' 而非 'knowl_pad'"
  echo "$hits" | sed 's/^/   /'
  violations=$((violations + 1))
fi

# ── 数据目录禁止写法（应为 .knowlpad，无分隔符）──────
check '.knowl-pad'  '数据目录应为 .knowlpad（无分隔符）'
check '.knowl_pad'  '数据目录应为 .knowlpad（无分隔符）'

if [[ $violations -gt 0 ]]; then
  echo ""
  echo "命名一致性检查失败，共 $violations 类违规。规范见 PRD §0.2。"
  exit 1
fi

echo "✅ 命名一致性检查通过"
