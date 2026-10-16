/**
 * 行定位的**纯 DOM 实现**（FR-LINK-13；列级区间见 FR-SEARCH-07）。
 *
 * 单独成文件的原因有二：① 适配器文件已贴近 CODE-11 的 200 行线；
 * ② 这里是纯函数，可以直接单测"选中哪一行/越界怎么收敛"，不必 mount 编辑器。
 *
 * 约定：line 为 **1-based**，与索引库的 line 同口径；`range` 是**相对该行文本**的字符偏移
 * （0 基、左闭右开），与命中片段契约的 highlightStart/highlightEnd 同口径。
 * 返回是否真的定位成功 —— 拿不到可编辑元素时返回 false，由调用方**如实降级**。
 */

/** 行内区间（相对该行文本；与 `SearchSnippet.highlightStart/End` 同口径）。 */
export interface RevealRange {
  start: number;
  end: number;
}

/** 把区间夹进行长；非法或空区间返回 null（调用方回退成"整行选中"）。 */
function clampRange(length: number, range: RevealRange): RevealRange | null {
  const start = Math.min(Math.max(Math.trunc(range.start), 0), length);
  const end = Math.min(Math.max(Math.trunc(range.end), 0), length);
  return end > start ? { start, end } : null;
}

export function revealLineIn(
  host: HTMLElement | null,
  value: string,
  line: number,
  range?: RevealRange,
): boolean {
  if (!host) return false;
  const ta = host.querySelector('textarea');
  if (!ta) return false;
  const lines = value.split('\n');
  if (lines.length === 0) return false;
  const idx = Math.min(Math.max(Math.trunc(line), 1), lines.length) - 1;
  let lineStart = 0;
  for (let i = 0; i < idx; i += 1) lineStart += lines[i].length + 1;
  // 有合法列区间就**精确选中那一处**（FR-SEARCH-07 的字面口径）；否则退回整行（FR-LINK-13 的既有行为）
  const hit = range ? clampRange(lines[idx].length, range) : null;
  const start = hit ? lineStart + hit.start : lineStart;
  const end = hit ? lineStart + hit.end : lineStart + lines[idx].length;
  ta.focus();
  ta.setSelectionRange(start, end);
  // 按行高估算滚动位置，让目标行大致落在视口上三分之一处
  const lineHeight = Number.parseFloat(getComputedStyle(ta).lineHeight || '') || 20;
  ta.scrollTop = Math.max(0, idx * lineHeight - ta.clientHeight / 3);
  return true;
}
