/**
 * Markdown 代码块的**轻量着色**（围栏标了 ` ```markdown ` 的示例）：标题 / 引用 / 列表标记 / 行内代码 / 链接 /
 * 强调。目的只是让「笔记里贴的一段 Markdown 示例」看起来有结构，**不是**完整的 Markdown 解析器。
 *
 * **行内代码不做语法高亮**：技术方案 §2.2 明写「inline 代码（单反引号）不做语法高亮 —— 内容过短无法可靠
 * 判定语言，主流实现亦默认关闭」。这里只把它标成代码色。
 */
import { escapeHtml } from './escapeHtml';

const CLASS_FENCE = 'kp-hl-code';
const CLASS_HEADING = 'kp-hl-heading';
const CLASS_QUOTE = 'kp-hl-quote';
const CLASS_LIST = 'kp-hl-list';
const CLASS_CODE = 'kp-hl-code';
const CLASS_LINK = 'kp-hl-link';
const CLASS_URL = 'kp-hl-url';
const CLASS_STRONG = 'kp-hl-strong';
const CLASS_EM = 'kp-hl-em';

const FENCE = /^\s*(?:```|~~~)/;
const HEADING = /^(#{1,6})(\s+)([\s\S]*)$/;
const QUOTE = /^(\s*(?:>\s?)+)([\s\S]*)$/;
const LIST = /^(\s*(?:[-*+]|\d+[.)])\s+)([\s\S]*)$/;
const INLINE_CODE = /^`([^`]+)`/;
const LINK = /^\[([^\]]*)\]\(([^)]*)\)/;
const STRONG = /^\*\*([^*]+)\*\*/;
const EM = /^\*([^*]+)\*/;

function span(cls: string, text: string): string {
  return '<span class="' + cls + '">' + escapeHtml(text) + '</span>';
}

/** 行内片段：只认「行内代码 / 链接 / 加粗 / 斜体」四种，其余按纯文本转义。 */
function renderInline(text: string): string {
  const out: string[] = [];
  let plain = '';
  const flush = (): void => {
    if (plain !== '') {
      out.push(escapeHtml(plain));
      plain = '';
    }
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    const code = INLINE_CODE.exec(rest);
    if (code !== null) {
      flush();
      out.push(span(CLASS_CODE, code[0]));
      i += code[0].length;
      continue;
    }
    const link = LINK.exec(rest);
    if (link !== null) {
      flush();
      out.push(span(CLASS_LINK, link[1]));
      out.push(span(CLASS_URL, '(' + link[2] + ')'));
      i += link[0].length;
      continue;
    }
    const strong = STRONG.exec(rest);
    if (strong !== null) {
      flush();
      out.push(span(CLASS_STRONG, strong[1]));
      i += strong[0].length;
      continue;
    }
    const em = EM.exec(rest);
    if (em !== null) {
      flush();
      out.push(span(CLASS_EM, em[1]));
      i += em[0].length;
      continue;
    }
    plain += text[i];
    i += 1;
  }
  flush();
  return out.join('');
}

function renderLine(line: string): string {
  if (FENCE.test(line)) return span(CLASS_FENCE, line);
  const heading = HEADING.exec(line);
  if (heading !== null) {
    return span(CLASS_HEADING, heading[1] + heading[2]) + renderInline(heading[3]);
  }
  const quote = QUOTE.exec(line);
  if (quote !== null) return span(CLASS_QUOTE, quote[1]) + renderInline(quote[2]);
  const list = LIST.exec(line);
  if (list !== null) return span(CLASS_LIST, list[1]) + renderInline(list[2]);
  return renderInline(line);
}

/** 逐行着色（保留换行：代码块的空白与缩进是内容的一部分，不能被吃掉）。 */
export function highlightMarkdownSpans(code: string): string {
  return code.split('\n').map(renderLine).join('\n');
}