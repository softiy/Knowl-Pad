/**
 * 「类代码」语言的**线性词法扫描**：纯文本 → 带 `kp-hl-*` class 的片段（DEBT-18 / FR-EDITOR-44）。
 *
 * 三条硬约束决定了这份实现为什么长这样：
 * ① **ED-06**：不得引入 shiki / highlight.js / prismjs 等第三方高亮库 ⇒ 自己扫；而且必须**单遍、无回溯**
 *    （正则灾难面是 D-07 / T-05 的前车之鉴，所以这里连正则都没用，全部是字符码判断）；
 * ② **FR-EDITOR-44**：高亮器是「纯文本 → span」的转换，**不产生任何可执行路径** ⇒ 这里只做字符分类，
 *    所有文本先 `escapeHtml` 再包裹 span（**HL-02**：转义必须发生在包裹之前）；
 * ③ 未知语言 / 超大代码块的**降级**由 `highlight.ts` 统一负责（**HL-03/HL-07**）。
 *
 * 结构：`nextToken` 是唯一的判别入口（返回「class + 长度」或 null），每种词法一个助手函数 —— 这样
 * 单函数行数受控（CODE-11 与 eslint 的 `max-lines-per-function` 同源约束，不豁免）。
 *
 * 已知取舍（均为「配色可能不准」，不影响安全与结构）：
 * - JS/TS 模板字面量里的 `${...}` 整段按字符串着色，不做嵌套；
 * - Rust 生命周期 `'a` 与字符字面量 `'x'` 用长度启发式区分（见 `charLiteralToken`）；
 * - JSON 的负号不算数字的一部分（`-` 走普通文本）；关键字/标识符判断只看 ASCII。
 */
import { escapeHtml } from './escapeHtml';

/** 一门语言的词法参数。集合都在模块级只建一次（HL-04 的同源要求：别按块重建）。 */
export interface LangConfig {
  /** 关键字（区分大小写）。 */
  readonly keywords: ReadonlySet<string>;
  /** 行注释起始串（`//`、`#`）；null = 没有行注释。 */
  readonly lineComment: string | null;
  /** `#` 只在行首或空白后才算注释（bash：`a#b` 里的 `#` 是普通字符）。 */
  readonly hashCommentBoundary?: boolean;
  /** 块注释的 [开, 闭]；null = 没有。 */
  readonly blockComment: readonly [string, string] | null;
  /** 单行字符串定界符（按「允许反斜杠转义」处理，遇换行即止）。 */
  readonly strings: readonly string[];
  /** 可以跨行的字符串定界符（JS 模板字面量）。 */
  readonly multiline?: readonly string[];
  /** Rust：`'x'` 字符字面量与 `'a` 生命周期要区分。 */
  readonly charLiteral?: boolean;
  /** Rust：`r"..."` / `r#"..."#` 原始字符串。 */
  readonly rawStrings?: boolean;
  /** bash：`$VAR` / `${VAR}`。 */
  readonly dollarVars?: boolean;
  /** 字面量（true / false / null）。 */
  readonly literals?: ReadonlySet<string>;
}

/** 一个词法片段：`cls === null` 表示「按普通文本处理」（例如非关键字的标识符）。 */
interface Token {
  readonly cls: string | null;
  readonly len: number;
}

const KEYWORD = 'kp-hl-keyword';
const STRING = 'kp-hl-string';
const COMMENT = 'kp-hl-comment';
const NUMBER = 'kp-hl-number';
const LITERAL = 'kp-hl-literal';
const VAR = 'kp-hl-var';

function isUpper(c: number): boolean {
  return c >= 65 && c <= 90;
}
function isLower(c: number): boolean {
  return c >= 97 && c <= 122;
}
function isDigit(c: number): boolean {
  return c >= 48 && c <= 57;
}
function isHex(c: number): boolean {
  return isDigit(c) || (c >= 65 && c <= 70) || (c >= 97 && c <= 102);
}
/** 标识符起始：`A-Z a-z _ $`（与 JS 一致；Rust 的 `_` 亦覆盖）。 */
function isIdentStart(c: number): boolean {
  return isUpper(c) || isLower(c) || c === 95 || c === 36;
}
function isIdentPart(c: number): boolean {
  return isIdentStart(c) || isDigit(c);
}
function isSpace(ch: string): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}

/** 块注释（含未闭合：一路吃到文末）。 */
function blockCommentToken(code: string, i: number, cfg: LangConfig): Token | null {
  if (cfg.blockComment === null) return null;
  const [open, close] = cfg.blockComment;
  if (!code.startsWith(open, i)) return null;
  const end = code.indexOf(close, i + open.length);
  const len = (end === -1 ? code.length : end + close.length) - i;
  return { cls: COMMENT, len };
}

/** 行注释（bash 的 `#` 需要左边界）。 */
function lineCommentToken(code: string, i: number, cfg: LangConfig): Token | null {
  const marker = cfg.lineComment;
  if (marker === null) return null;
  if (!code.startsWith(marker, i)) return null;
  if (cfg.hashCommentBoundary === true && i > 0 && !isSpace(code[i - 1])) return null;
  const nl = code.indexOf('\n', i);
  return { cls: COMMENT, len: (nl === -1 ? code.length : nl) - i };
}

/** Rust 原始字符串 `r"..."` / `r#"..."#`。 */
function rawStringToken(code: string, i: number, cfg: LangConfig): Token | null {
  if (cfg.rawStrings !== true || code[i] !== 'r') return null;
  let k = i + 1;
  let hashes = 0;
  while (code[k] === '#') {
    hashes += 1;
    k += 1;
  }
  if (code[k] !== '"') return null;
  const close = '"' + '#'.repeat(hashes);
  const end = code.indexOf(close, k + 1);
  return { cls: STRING, len: (end === -1 ? code.length : end + close.length) - i };
}

/** Rust 字符字面量 `'x'` / `'\\n'`；生命周期 `'a` 返回 null（交给普通文本）。 */
function charLiteralToken(code: string, i: number, cfg: LangConfig): Token | null {
  if (cfg.charLiteral !== true || code[i] !== "'") return null;
  const c1 = code[i + 1];
  if (c1 === undefined || c1 === '\n') return null;
  if (c1 !== '\\') return code[i + 2] === "'" ? { cls: STRING, len: 3 } : null;
  for (let k = i + 2; k < Math.min(code.length, i + 12); k += 1) {
    if (code[k] === "'") return { cls: STRING, len: k - i + 1 };
    if (code[k] === '\n') return null;
  }
  return null;
}

/** 字符串（单行 / 可跨行）；未闭合时吃到行尾（跨行字符串吃到文末）。 */
function stringToken(code: string, i: number, cfg: LangConfig): Token | null {
  const ch = code[i];
  const mayCrossLines = cfg.multiline?.includes(ch) === true;
  if (!cfg.strings.includes(ch) && !mayCrossLines) return null;
  let j = i + 1;
  while (j < code.length) {
    if (code[j] === '\\') {
      j += 2;
      continue;
    }
    if (code[j] === ch || (mayCrossLines !== true && code[j] === '\n')) break;
    j += 1;
  }
  return { cls: STRING, len: Math.max(1, (code[j] === ch ? j + 1 : j) - i) };
}

/** bash 的 `$VAR` / `${VAR}`。 */
function dollarVarToken(code: string, i: number, cfg: LangConfig): Token | null {
  if (cfg.dollarVars !== true || code[i] !== '$') return null;
  let j = i + 1;
  if (code[j] === '{') {
    const end = code.indexOf('}', j + 1);
    return { cls: VAR, len: (end === -1 ? code.length : end + 1) - i };
  }
  while (isIdentPart(code.charCodeAt(j))) j += 1;
  return j > i + 1 ? { cls: VAR, len: j - i } : null;
}

/** 数字：支持 `0x` / `0b` / `_` 分隔 / 小数 / 指数。 */
function numberToken(code: string, i: number): Token | null {
  if (!isDigit(code.charCodeAt(i))) return null;
  let j = i;
  if (code[j] === '0' && (code[j + 1] === 'x' || code[j + 1] === 'X')) {
    j += 2;
    while (isHex(code.charCodeAt(j)) || code[j] === '_') j += 1;
    return { cls: NUMBER, len: j - i };
  }
  if (code[j] === '0' && (code[j + 1] === 'b' || code[j + 1] === 'B')) {
    j += 2;
    while (code[j] === '0' || code[j] === '1' || code[j] === '_') j += 1;
    return { cls: NUMBER, len: j - i };
  }
  while (isDigit(code.charCodeAt(j)) || code[j] === '_') j += 1;
  if (code[j] === '.' && isDigit(code.charCodeAt(j + 1))) {
    j += 1;
    while (isDigit(code.charCodeAt(j)) || code[j] === '_') j += 1;
  }
  if (code[j] === 'e' || code[j] === 'E') {
    let k = j + 1;
    if (code[k] === '+' || code[k] === '-') k += 1;
    if (isDigit(code.charCodeAt(k))) {
      j = k;
      while (isDigit(code.charCodeAt(j)) || code[j] === '_') j += 1;
    }
  }
  return { cls: NUMBER, len: j - i };
}

/** 标识符 → 关键字 / 字面量 / 普通（普通返回 `cls: null`，由调用方并入纯文本）。 */
function identifierToken(code: string, i: number, cfg: LangConfig): Token | null {
  if (!isIdentStart(code.charCodeAt(i))) return null;
  let j = i + 1;
  while (isIdentPart(code.charCodeAt(j))) j += 1;
  const word = code.slice(i, j);
  if (cfg.keywords.has(word)) return { cls: KEYWORD, len: j - i };
  if (cfg.literals?.has(word) === true) return { cls: LITERAL, len: j - i };
  return { cls: null, len: j - i };
}

/** 唯一的判别入口：按「注释 → 字符串 → 变量 → 数字 → 标识符」的顺序试，全部不中返回 null。 */
function nextToken(code: string, i: number, cfg: LangConfig): Token | null {
  return (
    blockCommentToken(code, i, cfg) ??
    lineCommentToken(code, i, cfg) ??
    rawStringToken(code, i, cfg) ??
    charLiteralToken(code, i, cfg) ??
    stringToken(code, i, cfg) ??
    dollarVarToken(code, i, cfg) ??
    numberToken(code, i) ??
    identifierToken(code, i, cfg)
  );
}

/**
 * 把代码文本切成 `kp-hl-*` 片段。**单遍、无回溯、无正则**：每个字符最多看一次。
 *
 * 输出只有两种形态：转义后的文本、`<span class="kp-hl-x">转义文本</span>`（class 是本文件的常量，
 * 不含任何来自输入的插值 —— 这是 FR-EDITOR-44「无执行路径」在实现层面的落点）。
 */
export function highlightCodeSpans(code: string, cfg: LangConfig): string {
  const out: string[] = [];
  let plain = '';
  const flush = (): void => {
    if (plain !== '') {
      out.push(escapeHtml(plain));
      plain = '';
    }
  };
  const span = (cls: string, text: string): void => {
    flush();
    out.push('<span class="' + cls + '">' + escapeHtml(text) + '</span>');
  };
  let i = 0;
  while (i < code.length) {
    const tok = nextToken(code, i, cfg);
    if (tok === null) {
      plain += code[i];
      i += 1;
      continue;
    }
    const text = code.slice(i, i + tok.len);
    if (tok.cls === null) plain += text;
    else span(tok.cls, text);
    i += tok.len;
  }
  flush();
  return out.join('');
}