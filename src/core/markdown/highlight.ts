/**
 * 渲染态代码块高亮的**公共入口**（DEBT-18 / FR-EDITOR-44 P0 / FR-EDITOR-03）。
 *
 * 分工：
 * - 本文件：语言判定、**降级**（未知语言 / 超长块）、行数统计、对外 API；
 * - `highlightLangs.ts`：五类语言的最小配置（为什么是这组见那里的注释）；
 * - `highlightCode.ts`：线性词法扫描（纯文本 → `kp-hl-*` span）；
 * - `highlightMarkdown.ts`：```markdown 围栏里的轻量着色。
 *
 * 三条约束的落点（技术方案 §2.2 「代码高亮方案」+ §7.4 ED-06）：
 * - **HL-01**：本函数的输出**仍要**经 `sanitizeHtml()`（调用方 `renderer.ts` 的管线保证），
 *   不得因为「内容是自家生成的」而跳过净化；
 * - **HL-03**：未支持的语言**降级为纯文本转义**，不报错、不吞内容、不做语言自动探测；
 * - **HL-07**：超过 `HIGHLIGHT_LINE_LIMIT` 行直接降级 —— 高亮是同步的，大块会阻塞主线程。
 *
 * **class 命名**：仓库既有自研 class 都用 `kp-` 前缀（`kp-link` / `kp-tag` / `kp-remote-image`），这里沿用
 * `kp-hl-*`。技术方案 §2.2 的示例代码用的是 `@lezer/highlight` 的 `classHighlighter`（输出 `tok-*`），
 * 那是 **M8** 换 Lezer 时的事；届时把 Lezer 的 tag 映射到这套 class（或整体替换 CSS）即可，两套口径已记在
 * DEBT-18 的处置说明里。
 */
import { escapeHtml } from './escapeHtml';
import { highlightCodeSpans } from './highlightCode';
import { highlightMarkdownSpans } from './highlightMarkdown';
import { CODE_LANGS } from './highlightLangs';

/** 超过此行数的代码块跳过高亮（**HL-07**；技术方案 §2.2 的 2000 行口径）。 */
export const HIGHLIGHT_LINE_LIMIT = 2000;

/** 支持的语言（规范名；别名见 `ALIASES`）。 */
export type HighlightLanguage = 'js' | 'ts' | 'rust' | 'json' | 'bash' | 'markdown';

/** 对外的语言清单（文档/用例用；与 `ALIASES` 的取值集合一致）。 */
export const HIGHLIGHT_LANGUAGES: readonly HighlightLanguage[] = ['js', 'ts', 'rust', 'json', 'bash', 'markdown'];

/** 别名 → 规范名。键**必须**小写（`resolveLanguage` 会先 `toLowerCase`）。 */
const ALIASES: Readonly<Record<string, HighlightLanguage>> = {
  js: 'js',
  javascript: 'js',
  jsx: 'js',
  mjs: 'js',
  cjs: 'js',
  ts: 'ts',
  typescript: 'ts',
  tsx: 'ts',
  rust: 'rust',
  rs: 'rust',
  json: 'json',
  jsonc: 'json',
  bash: 'bash',
  sh: 'bash',
  shell: 'bash',
  zsh: 'bash',
  markdown: 'markdown',
  md: 'markdown',
};

/**
 * 从 fence 的 info string 解析语言：取**首段**并小写（技术方案 §2.2：` ```rust ` 的 `rust`，
 * info 可能带额外参数如 ` ```js title="x" `）。不支持/为空返回 null。
 */
export function resolveLanguage(info: string): HighlightLanguage | null {
  const first = info.trim().split(/\s+/)[0]?.toLowerCase() ?? '';
  return ALIASES[first] ?? null;
}

/** 该 info string 是否命中支持的语言（fence 渲染器据此决定语言 class）。 */
export function isSupportedLanguage(info: string): boolean {
  return resolveLanguage(info) !== null;
}

/**
 * 行数（HL-07 的口径：**尾换行不算多一行** —— markdown-it 的 fence `token.content` 通常带尾 `\n`，
 * 若按「换行数 + 1」算，2000 行的块会变成 2001 行而被误判超限）。空串算 0 行。
 */
function countLines(code: string): number {
  if (code === '') return 0;
  let newlines = 0;
  for (let i = 0; i < code.length; i += 1) {
    if (code[i] === '\n') newlines += 1;
  }
  return code.endsWith('\n') ? newlines : newlines + 1;
}

/**
 * 把一段代码文本渲染成**可安全插入 DOM 的 HTML 片段**（只有文本节点与 `kp-hl-*` span）。
 *
 * 签名与技术方案 §2.2 的示例一致（`highlightCodeBlock(code, info)`），便于 M8 替换实现时调用方不动。
 * 降级统一在这里：语言不支持（HL-03）或行数超限（HL-07）⇒ 纯文本转义。
 */
export function highlightCodeBlock(code: string, info: string): string {
  const lang = resolveLanguage(info);
  if (lang === null) return escapeHtml(code);
  if (countLines(code) > HIGHLIGHT_LINE_LIMIT) return escapeHtml(code);
  if (lang === 'markdown') return highlightMarkdownSpans(code);
  const cfg = CODE_LANGS[lang];
  if (cfg === undefined) return escapeHtml(code);
  return highlightCodeSpans(code, cfg);
}