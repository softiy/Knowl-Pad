/**
 * **自研**的 markdown-it inline 规则：wikilink 与标签（DEBT-34 的另一半；FR-EDITOR-02/25/27、FR-TAG-05）。
 *
 * 为什么自己写而不是装插件（两条都是硬约束）：
 * ① **零新依赖**（本批约束）：社区插件要么引入新包，要么把本仓不需要的语法一并绑进来；
 * ② **口径必须与 Rust 解析器一致**：`crates/kp-domain/src/md_parse/{wikilink,tag}.rs` 才是
 *    "什么算一个链接/标签"的唯一权威（MD-WL-01~09 / MD-TAG-01~06）。渲染侧宽一格就会出现
 *    "看得见、点不了"的假标签。这里只做**形态识别**，**不判有效性** —— 有效性由索引阶段的
 *    链接裁决产出（MD-WL-03），渲染侧只负责把目标写进属性，交给上层打状态类。
 *
 * 产出的是**中转属性**（两个名字都已在 `sanitize.ts` 的 ADD_ATTR 白名单里，**不要动安全配置**）：
 * - `data-kp-link` + `data-kp-kind`（`link` / `embed`）+ `data-kp-anchor`（有锚点才输出）；
 * - `data-kp-tag`（**原文大小写**，与 MD-TAG-05 一致）。
 * 最终类名、点击与导航由上层决定（`resolveWikilinks` 打悬空态类 → `MarkdownView` 抛事件 → `EditorView` 路由）。
 *
 * **不做的事**（各有去向，避免"看起来支持了"）：
 * - 不渲染嵌入（`![[...]]`，FR-EDITOR-29 是 P1）：只标 `data-kp-kind="embed"`，点击与普通链接同路；
 * - 不渲染 Block ID（`^block`）：只有嵌入语法用到它，同上；
 * - frontmatter 不进 inline 规则（markdown-it 本就不解析它）。
 */

import type { MarkdownIt, StateInline, Token } from 'markdown-it';

/** 链接形态（与 Rust 的 `LinkKind` 同口径）。 */
export type KpLinkKind = 'link' | 'embed';

/** 标签允许的字符（MD-TAG-01；与 Rust 的 `is_tag_char` 逐字对应）。 */
function isTagChar(ch: string): boolean {
  return !/\s/.test(ch) && (/[\p{L}\p{N}]/u.test(ch) || ch === '_' || ch === '-' || ch === '/');
}

/**
 * 起始边界（MD-TAG-01 的 `is_start_boundary`：行首/空白/常见标点 ⇒ `abc#def` 不是标签）。
 *
 * 半角之外**必须覆盖全角**：中文正文里 `：`、`，`、`。`、`（` 后面接标签是最常见的写法之一，
 * 漏掉它们会让"渲染成标签但索引里没有"的假标签出现（实测：`中文标点后：#标签` 一度渲染成纯文本）。
 */
const TAG_BOUNDARY_PUNCT = '(,;:!?\'"[{：（，。；：！？、（）《》“”‘’【】「」';

function isTagBoundary(before: string): boolean {
  return before === '' || /\s/.test(before) || TAG_BOUNDARY_PUNCT.includes(before);
}

/** 去掉首尾 `/` 并折叠空段（`#/a//b/` → `a/b`，与 Rust 的 `clean` 同口径）。 */
function cleanTag(raw: string): string {
  return raw.split('/').filter((seg) => seg !== '').join('/');
}

/**
 * 解析 `[[目标#锚点|别名]]` 的**内部内容**（不含双方括号）。
 *
 * 顺序与 MD-WL-01/WL-04 一致：先按**第一个** `|` 切别名，再按**第一个** `#` 切锚点。
 * 目标里的 `#` 因此按"锚点分隔符"解释（与解析器一致）。
 */
export function parseWikilinkBody(body: string): { target: string; anchor: string; alias: string } | null {
  const bar = body.indexOf('|');
  const alias = bar >= 0 ? body.slice(bar + 1).trim() : '';
  const head = bar >= 0 ? body.slice(0, bar) : body;
  const hash = head.indexOf('#');
  const anchor = hash >= 0 ? head.slice(hash + 1).trim() : '';
  const target = (hash >= 0 ? head.slice(0, hash) : head).trim();
  if (!target) return null;
  return { target, anchor, alias };
}

/** 该位置是否是**未被转义**的 `[[`（MD-WL-08：奇数个反斜杠 = 被转义）。 */
function isUnescapedOpen(src: string, pos: number): boolean {
  if (src.charCodeAt(pos) !== 0x5b || src.charCodeAt(pos + 1) !== 0x5b) return false;
  let backslashes = 0;
  let k = pos - 1;
  while (k >= 0 && src.charCodeAt(k) === 0x5c) {
    backslashes += 1;
    k -= 1;
  }
  return backslashes % 2 === 0;
}

/** 该位置是否是一个**标签**（返回规范化后的名字；null 表示不是）。 */
export function tagAt(src: string, pos: number): string | null {
  if (src.charCodeAt(pos) !== 0x23) return null;
  let i = pos + 1;
  let name = '';
  while (i < src.length && isTagChar(src[i] ?? '')) {
    name += src[i];
    i += 1;
  }
  const cleaned = cleanTag(name);
  // MD-TAG-01：不得以数字开头（纯数字一律不算）
  if (!cleaned || /^\d/.test(cleaned)) return null;
  return cleaned;
}

/** 该位置是否可以**开始**一个标签（边界判定；行首/空白/常见标点）。 */
function canStartTag(src: string, pos: number): string | null {
  // `#` 前面若是普通字符，则它是"词中井号"（`abc#def` / URL 片段），不是标签
  if (!isTagBoundary(pos === 0 ? '' : (src[pos - 1] ?? ''))) return null;
  return tagAt(src, pos);
}

/** `[[...]]` → 一个带 `data-kp-link` 的 span（`![[...]]` 同规则，多一个 kind）。 */
function wikilinkInline(state: StateInline, silent: boolean): boolean {
  const start = state.pos;
  const embed = state.src.charCodeAt(start) === 0x21; // `!`
  const open = embed ? start + 1 : start;
  if (!isUnescapedOpen(state.src, open)) return false;
  const close = state.src.indexOf(']]', open + 2);
  if (close < 0) return false;
  const body = state.src.slice(open + 2, close);
  // 链接体不跨行（与解析器同口径：一行之内的引用）
  if (body.includes('\n')) return false;
  const parsed = parseWikilinkBody(body);
  if (!parsed) return false;
  if (silent) return true;

  const token = state.push('kp_link', '', 0);
  token.content = parsed.target;
  token.meta = {
    target: parsed.target,
    anchor: parsed.anchor,
    kind: (embed ? 'embed' : 'link') satisfies KpLinkKind,
    label: parsed.alias || parsed.target,
  };
  state.pos = close + 2;
  return true;
}

/** `#标签` → 一个带 `data-kp-tag` 的 span。 */
function tagInline(state: StateInline, silent: boolean): boolean {
  const cleaned = canStartTag(state.src, state.pos);
  if (!cleaned) return false;
  if (silent) return true;
  // 前进量按**原文**长度算：`#a/ ` 的规范名是 `a`（尾部斜杠被 clean 掉），但字符要全部吃掉
  const raw = rawTagLength(state.src, state.pos);
  const token = state.push('kp_tag', '', 0);
  token.content = cleaned;
  token.meta = { tag: cleaned };
  state.pos += raw + 1;
  return true;
}

/** 从 `#` 后开始连续吃掉的**原文字符数**（`cleanTag` 之前的口径）。 */
function rawTagLength(src: string, pos: number): number {
  let i = pos + 1;
  while (i < src.length && isTagChar(src[i] ?? '')) i += 1;
  return i - pos - 1;
}

/** 渲染 `kp_link`：目标与锚点写进中转属性，显示文本用别名或目标原文。 */
function renderKpLink(tokens: Token[], idx: number): string {
  const meta = (tokens[idx]?.meta ?? {}) as { target?: string; anchor?: string; kind?: string; label?: string };
  const target = meta.target ?? '';
  const anchor = meta.anchor ? ` data-kp-anchor="${escapeAttr(meta.anchor)}"` : '';
  const label = meta.label ?? target;
  return (
    `<span class="kp-link" data-kp-link="${escapeAttr(target)}"` +
    ` data-kp-kind="${meta.kind === 'embed' ? 'embed' : 'link'}"${anchor}>` +
    `${escapeText(label)}</span>`
  );
}

/** 渲染 `kp_tag`：`data-kp-tag` 用**原文大小写**（MD-TAG-05）。 */
function renderKpTag(tokens: Token[], idx: number): string {
  const tag = tokens[idx]?.content ?? '';
  return `<span class="kp-tag" data-kp-tag="${escapeAttr(tag)}">#${escapeText(tag)}</span>`;
}

/** 属性值转义（自足实现：这两个函数只处理属性/文本，不引入新的依赖）。 */
function escapeAttr(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** 文本节点转义（`<` 必须转义：否则 `[[a<b]]` 会变成真标签）。 */
function escapeText(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/**
 * 把两条规则与两个渲染器挂到实例上（`renderer.ts` 在模块加载时调用一次）。
 *
 * 实现要点（**踩过两次坑，都在这几行附近**）：
 * ① 默认链的第 0 条就是 `text`（`["text","linkify","newline",…,"image",…]`），而它的终止符
 *    本来就含 `!`、`[`、`#`（`\n ! # $ % & * + - : < = > @ [ \\ ] ^ _ ` { } ~`）——
 *    **所以不要自己切分、更不要替换 `text`**：任何自作聪明的 text 实现都会把 `![alt](url)` 的 `!`
 *    当普通文本吃掉，于是**图片整条降级为纯文本**（renderer 的 7 条用例集体变红，实测）。
 * ② 正确做法是**在 `text` 之前插两条识别规则**；`before('text', …)` 是**前插**，
 *    连续两次调用会得到"后插的在前"，因此先插 `kp_link`、再插 `kp_tag`，链上顺序才是
 *    `kp_tag` → `kp_link` → `escape` → `text`。
 * ③ 转义（`\[[` 不算链接）交给链上本来就有的 `escape` 规则：它把 `\[` 作为文本产出，
 *    `text` 再从 `[` 继续 —— 此时 `isUnescapedOpen` 看到的前一个字符正是那个反斜杠，判定正确。
 */
export function registerKpInline(md: MarkdownIt): void {
  md.inline.ruler.before('text', 'kp_link', wikilinkInline);
  md.inline.ruler.before('text', 'kp_tag', tagInline);
  // 刻意**不**改 text：见上方 ① —— 它本来就停在 `!`/`[`/`#`，替身写不好会连累图片
  md.renderer.rules['kp_link'] = (tokens, idx) => renderKpLink(tokens, idx);
  md.renderer.rules['kp_tag'] = (tokens, idx) => renderKpTag(tokens, idx);
}
