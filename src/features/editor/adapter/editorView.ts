/**
 * CodeMirror 6 视图的**最小结构化接面**（md-editor-v3 6.x 的内核）。
 *
 * 为什么不用 `@codemirror/view` 的类型：本项目不直接依赖 `@codemirror/*`（package.json 不新增依赖），
 * 适配层只用得到"文档行 + 选区事务 + 聚焦"这几个成员，用结构类型 + 运行时收窄即可 ——
 * 既过得了 strict，也不会把内核细节扩散出去（M8 换内核时这里就是唯一的桥）。
 */
import { revealOffsets, type RevealRange } from './revealLine';

interface LineHandle {
  readonly from: number;
  readonly to: number;
}

interface DocLike {
  readonly lines: number;
  line(number: number): LineHandle;
  toString(): string;
}

export interface EditorViewLike {
  readonly state: { readonly doc: DocLike };
  dispatch(spec: {
    selection?: { anchor: number; head: number } | undefined;
    scrollIntoView?: boolean | undefined;
    changes?: readonly { from: number; to: number; insert: string }[] | undefined;
  }): void;
  focus(): void;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/**
 * 把组件实例暴露的 `getEditorView()` 收窄成结构接面。
 * 形状不符（旧内核 / 尚未挂载）时返回 null —— 调用方**如实降级**，绝不假装定位成功。
 */
export function editorViewOf(exposed: unknown): EditorViewLike | null {
  if (!isRecord(exposed) || typeof exposed.getEditorView !== 'function') return null;
  const readView = exposed.getEditorView as () => unknown;
  const view = readView();
  if (!isRecord(view) || typeof view.dispatch !== 'function' || typeof view.focus !== 'function') return null;
  if (!isRecord(view.state) || !isRecord(view.state.doc)) return null;
  const doc = view.state.doc;
  if (typeof doc.line !== 'function' || typeof doc.toString !== 'function' || typeof doc.lines !== 'number') return null;
  return view as unknown as EditorViewLike;
}

/**
 * 选中并滚动到目标区间（FR-LINK-13 整行 / FR-SEARCH-07 命中列区间）。
 * 走 CodeMirror 的选区事务而不是 textarea.setSelectionRange：后者在 CodeMirror 内核里根本不存在，
 * 这正是"只计数不选中"的根因（10.1/10.2/10.4）。
 */
export function revealInView(view: EditorViewLike, line: number, range?: RevealRange): boolean {
  const offsets = revealOffsets(view.state.doc, line, range);
  if (!offsets) return false;
  view.dispatch({ selection: { anchor: offsets.from, head: offsets.to }, scrollIntoView: true });
  return true;
}
