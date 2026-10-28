/**
 * 阅读态点击的**分派**（FR-EDITOR-25 / FR-EDITOR-27 / FR-TAG-05）。
 *
 * 判定（"该跳、该建、还是不动作"）在 `logic/linkRouting.ts`（本模块的**上一层** `features/editor/logic/`）的纯函数里；
 * 这里只负责把判定结果接到**具体的出口**上，并把出口本身做成**注入项**：
 *
 * ⚠️ 注释里**不要写"两点 + 斜杠"开头的相对模块路径**：门禁 16（SEC-02 路径封装扫描）会把它
 * 当成"前端自行拼接父目录路径"而判红 —— 这是**误报**（该扫描排除了模块导入语句，但**没排除注释**），
 * 已登记为 **DEBT-39**。改写方式：只写模块名 + 说清它在本 feature 的哪一层（本行就是这么写的）。
 *
 * - `openLink` / `createNote` / `openTag`：由上层（`WorkspaceLayout`）注入；
 * - 缺省回落：跳转走 `editor.reveal`（与反链面板/搜索结果**同一条既有通路**）、创建与标签只 `emit`。
 *
 * 这样编辑器域**不认识**文件树与标签域（FE-02），组件单独挂载时也依然能工作。
 */
import { clickIntent } from '../logic/linkRouting';

/**
 * 注入项一律是**取值函数**而不是值本身：组件的 props 可能在挂载后被替换
 * （`setProps` / 上层条件渲染），在 `setup` 里抄一份值就等于把它冻住 ——
 * 症状是"注入了回调却永远不被调用"（实测：4 条用例红，回调解构发生得太早）。
 */
export interface ClickRoutingOptions {
  /** 缺省跳转（通常是 `editor.reveal`）；缺省时不动作。 */
  fallbackOpen?: (() => ((target: string) => void) | undefined) | undefined;
  openLink?: (() => ((target: string, event?: MouseEvent) => void) | undefined) | undefined;
  createNote?: (() => ((name: string) => void) | undefined) | undefined;
  openTag?: (() => ((tag: string) => void) | undefined) | undefined;
  /** 没人处理创建时的出口（组件 emit）。 */
  emitCreate: (name: string) => void;
  /** 标签点击的出口（组件 emit）—— 无论是否注入 openTag 都会发，便于上层统一观测。 */
  emitTag: (tag: string) => void;
}

export interface ClickRouting {
  /** `event` 缺省 = 键盘激活（无修饰键，与"普通点击"同义）。 */
  onLink: (payload: { target: string; dangling: boolean }, event?: MouseEvent) => void;
  onTag: (tag: string) => void;
}

/** 组装两个事件处理器（组件只把它们挂到 MarkdownView 上）。 */
export function createClickRouting(options: ClickRoutingOptions): ClickRouting {
  function onLink(payload: { target: string; dangling: boolean }, event?: MouseEvent): void {
    const intent = clickIntent(payload, event ?? {});
    if (intent.kind === 'none') return;
    if (intent.kind === 'create') {
      const create = options.createNote?.();
      if (create) create(intent.name);
      else options.emitCreate(intent.name);
      return;
    }
    const open = options.openLink?.();
    if (open === undefined) {
      options.fallbackOpen?.()?.(intent.target);
      return;
    }
    // 只有「新标签页」意图才需要事件（Alt 分流）；普通跳转保持旧签名
    if (intent.kind === 'link-new-tab') open(intent.target, event);
    else open(intent.target);
  }

  function onTag(tag: string): void {
    if (!tag) return;
    options.openTag?.()?.(tag);
    options.emitTag(tag);
  }

  return { onLink, onTag };
}
