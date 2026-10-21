/**
 * M5 支持的**最小组**语言配置（DEBT-18 / FR-EDITOR-44）。
 *
 * 为什么是这一组（技术方案 §2.2 的「语言覆盖」说的是 M8 的 Lezer 方案：官方 `@lezer/*` 只有约 14 种，
 * 且明确写了 `@lezer/sql` / `@lezer/bash` **不存在**、`legacy-modes` 的 StreamParser 能否接 `highlightCode()`
 * 未验证 —— 那是 DEBT-09）。M5 的渲染态是自研扫描，不依赖任何 grammar，所以按**这套产品实际会用到**的
 * 五类取最小集：
 * - `js`/`ts`：本仓与 Tauri 生态的示例代码（含本仓自己的脚本）；
 * - `rust`：本仓后端（Tauri/Rust）的示例与报错片段；
 * - `json`：配置、IPC 载荷、fixture；
 * - `bash`/`sh`：命令与 CI 片段（PRD 的验收步骤大量是 shell/PS 混排）；
 * - `markdown`：笔记里嵌套的 Markdown 片段（FR-EDITOR-01 的语法示例）。
 * 其余语言按 **HL-03** 降级为纯文本转义（不报错、不吞内容）；扩语言只需往这里加配置。
 */
import type { LangConfig } from './highlightCode';

/** 空的集合（JSON 没有关键字）。 */
const NO_WORDS: ReadonlySet<string> = new Set<string>();

/** JS 关键字（含严格模式/模块语法）。 */
const JS_KEYWORDS: ReadonlySet<string> = new Set([
  'as', 'async', 'await', 'break', 'case', 'catch', 'class', 'const', 'continue', 'debugger',
  'default', 'delete', 'do', 'else', 'export', 'extends', 'finally', 'for', 'from', 'function',
  'get', 'if', 'import', 'in', 'instanceof', 'let', 'new', 'of', 'return', 'set', 'static',
  'super', 'switch', 'this', 'throw', 'try', 'typeof', 'var', 'void', 'while', 'with', 'yield',
]);

/** TS 在 JS 之外的关键字（高亮层面只需「看起来像关键字」，不追求编译器级精确）。 */
const TS_KEYWORDS: ReadonlySet<string> = new Set([
  ...JS_KEYWORDS,
  'abstract', 'declare', 'enum', 'implements', 'infer', 'interface', 'is', 'keyof', 'namespace',
  'private', 'protected', 'public', 'readonly', 'satisfies', 'type', 'unique', 'unknown',
]);

/** Rust 关键字。 */
const RUST_KEYWORDS: ReadonlySet<string> = new Set([
  'as', 'async', 'await', 'box', 'break', 'const', 'continue', 'crate', 'dyn', 'else', 'enum',
  'extern', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub',
  'ref', 'return', 'self', 'Self', 'static', 'struct', 'super', 'trait', 'type', 'unsafe', 'use',
  'where', 'while', 'yield',
]);

/** bash 的控制关键字/内建（最小集：够把命令与流程看清即可）。 */
const BASH_KEYWORDS: ReadonlySet<string> = new Set([
  'case', 'do', 'done', 'elif', 'else', 'esac', 'export', 'fi', 'for', 'function', 'if', 'in',
  'local', 'readonly', 'return', 'select', 'set', 'shift', 'source', 'then', 'unset', 'until', 'while',
]);

const JS_LITERALS: ReadonlySet<string> = new Set(['true', 'false', 'null', 'undefined', 'NaN', 'Infinity']);
const RUST_LITERALS: ReadonlySet<string> = new Set(['true', 'false']);
const JSON_LITERALS: ReadonlySet<string> = new Set(['true', 'false', 'null']);

/** JS / TS / JSX / TSX 共用一份扫描配置（差异只在关键字集合）。 */
const JS_CONFIG: LangConfig = {
  keywords: JS_KEYWORDS,
  lineComment: '//',
  blockComment: ['/*', '*/'] as const,
  strings: ["'", '"'],
  multiline: ['`'],
  literals: JS_LITERALS,
};

const TS_CONFIG: LangConfig = {
  ...JS_CONFIG,
  keywords: TS_KEYWORDS,
};

const RUST_CONFIG: LangConfig = {
  keywords: RUST_KEYWORDS,
  lineComment: '//',
  blockComment: ['/*', '*/'] as const,
  strings: ['"'],
  charLiteral: true,
  rawStrings: true,
  literals: RUST_LITERALS,
};

const JSON_CONFIG: LangConfig = {
  keywords: NO_WORDS,
  lineComment: null,
  blockComment: null,
  strings: ['"'],
  literals: JSON_LITERALS,
};

const BASH_CONFIG: LangConfig = {
  keywords: BASH_KEYWORDS,
  lineComment: '#',
  hashCommentBoundary: true,
  blockComment: null,
  strings: ["'", '"'],
  dollarVars: true,
};

/** 别名 → 扫描配置。键**必须**是小写（`highlight.ts` 会先做 `toLowerCase`）。 */
export const CODE_LANGS: Readonly<Record<string, LangConfig>> = {
  js: JS_CONFIG,
  javascript: JS_CONFIG,
  jsx: JS_CONFIG,
  mjs: JS_CONFIG,
  cjs: JS_CONFIG,
  ts: TS_CONFIG,
  typescript: TS_CONFIG,
  tsx: TS_CONFIG,
  rust: RUST_CONFIG,
  rs: RUST_CONFIG,
  json: JSON_CONFIG,
  jsonc: JSON_CONFIG,
  bash: BASH_CONFIG,
  sh: BASH_CONFIG,
  shell: BASH_CONFIG,
  zsh: BASH_CONFIG,
};