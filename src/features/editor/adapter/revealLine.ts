/**
 * 行定位的**纯计算部分**（FR-LINK-13；列级区间见 FR-SEARCH-07）。
 *
 * 单独成文件的原因有二：① 适配器文件贴近 CODE-11 的 200 行线；
 * ② 这里不碰 DOM / CodeMirror，可以直接单测"选中哪一行、越界怎么收敛"。
 *
 * 约定：line 为 **1-based**，与索引库的 line 同口径；`range` 是**相对该行文本**的字符偏移
 * （0 基、左闭右开），与命中片段契约的 highlightStart/highlightEnd 同口径。
 */

/** 行内区间（相对该行文本；与 `SearchSnippet.highlightStart/End` 同口径）。 */
export interface RevealRange {
  start: number;
  end: number;
}

/** 定位所需的最小文档面（CodeMirror 的 `Text` 满足它；单测用等价替身）。 */
export interface LineSource {
  readonly lines: number;
  line(number: number): { readonly from: number; readonly to: number };
}

/** 把区间夹进行长；非法或空区间返回 null（调用方回退成"整行选中"）。 */
function clampRange(length: number, range: RevealRange): RevealRange | null {
  const start = Math.min(Math.max(Math.trunc(range.start), 0), length);
  const end = Math.min(Math.max(Math.trunc(range.end), 0), length);
  return end > start ? { start, end } : null;
}

/**
 * 计算要选中/滚动的文档偏移区间。
 *
 * 有合法列区间就**精确选中那一处**（FR-SEARCH-07 的字面口径）；否则退回**整行**（FR-LINK-13 的既有行为）。
 * 行号越界按既有规则收敛到首/末行；文档为空（lines < 1）时返回 null，由调用方如实降级。
 */
export function revealOffsets(doc: LineSource, line: number, range?: RevealRange): { from: number; to: number } | null {
  if (doc.lines < 1) return null;
  const target = Math.min(Math.max(Math.trunc(line), 1), doc.lines);
  const textLine = doc.line(target);
  const hit = range ? clampRange(textLine.to - textLine.from, range) : null;
  return hit
    ? { from: textLine.from + hit.start, to: textLine.from + hit.end }
    : { from: textLine.from, to: textLine.to };
}
