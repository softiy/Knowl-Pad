import { currentTheme, type ResolvedTheme } from '@core/theme';
import { createApp, defineComponent, h, nextTick, ref, watch, type App, type Ref } from 'vue';
import { MdEditor } from 'md-editor-v3';
import 'md-editor-v3/lib/style.css';
import { editorViewOf, revealInView, type EditorViewLike } from './editorView';
import { findInText, replaceInText } from './findInText';
import {
  EditorFeaturePendingError,
  type EditorEvent,
  type EditorOptions,
  type FindOptions,
  type InFileMatch,
  type KpEditorAdapter,
  type ReplaceResult,
} from './types';

/** change 事件的最小间隔（ED-02：回调必须节流，≤ 60Hz）。 */
export const CHANGE_THROTTLE_MS = 16;

/**
 * MVP 内核适配器（md-editor-v3 6.x，内核是 CodeMirror 6）。
 *
 * - **ED-01**：`destroy()` 清定时器/主题观察者 + 卸载 Vue 实例 + 清处理器；
 * - **ED-02**：`change` 走 16ms 节流（尾部补发最后一次，保证不丢内容）；
 * - **ED-04**：不在这里绑定 keydown，快捷键由 `core/shortcut` 统一处理；
 * - **ED-06**：代码高亮用 md-editor-v3 内置，不额外引入高亮库。
 *
 * 定位/选中一律经内核的 `getEditorView()`（见 editorView.ts）：旧实现查 textarea，而 CodeMirror 内核
 * 根本没有 textarea ⇒ 永远定位失败、只计数不选中（10.1/10.2/10.4）。
 */
export class MdEditorV3Adapter implements KpEditorAdapter {
  private app: App<Element> | null = null;
  /** 宿主元素（仅用于视图就绪前的兜底聚焦；真正的编辑 DOM 在内核里）。 */
  private host: HTMLElement | null = null;
  /** 内核组件实例（只取 `getEditorView()`；形状在 editorView.ts 里运行时收窄）。 */
  private readonly editorRef = ref<unknown>(null);
  /** 内核主题跟着 `<html data-theme>` 走（8.3/8.5：切主题后编辑区必须跟着变色）。 */
  private theme: Ref<ResolvedTheme> | null = null;
  private themeObserver: MutationObserver | null = null;
  private value = '';
  private readonly handlers = new Map<EditorEvent, Set<(...args: unknown[]) => void>>();
  private applyExternal: ((next: string) => void) | null = null;
  private pendingValue: string | null = null;
  private pendingTimer: ReturnType<typeof setTimeout> | null = null;
  private lastEmitAt = 0;
  /** 程序化写入期间抑制 change（避免打开笔记就变脏）。 */
  private suppressChange = false;
  /**
   * setValue 之后内核视图要等一次渲染才回灌：在此之前 getValue/find 必须以刚写入的值为准，
   * 否则"打开/切换标签后立刻查找"会读到上一篇的内容。
   */
  private pendingExternal: string | null = null;

  mount(el: HTMLElement, opts: EditorOptions): void {
    this.host = el;
    if (this.app) {
      return; // 已挂载：复用实例（ED-01）
    }
    this.value = opts.initialValue ?? '';
    this.startThemeWatch(opts.theme);
    const Host = defineComponent({
      name: 'KpMdEditorHost',
      // 箭头函数捕获类实例的 this（避免 no-this-alias）
      setup: () => {
        const text = ref(this.value);
        this.applyExternal = (next: string) => {
          text.value = next;
        };
        watch(text, (next) => {
          // 程序化 setValue 已经把 this.value 设成同值：Vue 的 watcher 是异步的，
          // 用 suppressChange 挡不住，因此这里以「值是否真的变了」为准（打开笔记不应变脏）。
          if (next === this.value) return;
          this.value = next;
          this.scheduleChange(next);
        });
        return () =>
          h(MdEditor, {
            ref: this.editorRef,
            modelValue: text.value,
            'onUpdate:modelValue': (next: string) => {
              text.value = next;
            },
            preview: false, // 阅读/分屏的预览走 §7.2 的净化管线，不用内核自带预览
            theme: this.theme?.value ?? currentTheme(),
            placeholder: opts.placeholder ?? '',
            readonly: opts.readonly ?? false,
            // 内核自带的 Ctrl+S 也归一到 save 事件；与 core/shortcut 的绑定是同一处理函数，
            // 保存本身幂等（未变脏时不写盘），因此不会重复写。
            onSave: () => this.emit('save'),
          });
      },
    });
    this.app = createApp(Host);
    this.app.mount(el);
  }

  getValue(): string {
    const view = this.getView();
    this.flushChange();
    if (this.pendingExternal !== null) return this.pendingExternal;
    // 内核文档才是当前文本的权威来源：`value` 只在 change 节流/模型回灌路径上更新，可能慢一拍（16ms 内）。
    return view ? view.state.doc.toString() : this.value;
  }

  setValue(content: string): void {
    this.suppressChange = true;
    this.value = content;
    this.pendingExternal = content;
    this.applyExternal?.(content);
    this.suppressChange = false;
    void nextTick(() => {
      // 一次渲染后内核视图已按 modelValue 回灌；期间若又有新写入则以最新 pending 为准。
      if (this.value === content) this.pendingExternal = null;
    });
  }

  on(event: EditorEvent, handler: (...args: unknown[]) => void): void {
    const set = this.handlers.get(event) ?? new Set();
    set.add(handler);
    this.handlers.set(event, set);
  }

  insertLink(target: string, alias?: string): void {
    // 链接补全的插入需要「索引 + 光标事务」的补全流程（ED-03 在适配器之外），随 M8 的编辑器重写接入
    // （与勘误 D-39① 的 `FR-TAG-06` 同一去向）；这里显式失败而非静默无效——参数仅为满足契约签名。
    void target;
    void alias;
    throw new EditorFeaturePendingError('链接补全与插入', 'M8');
  }

  triggerSuggest(kind: 'link' | 'tag' | 'heading'): void {
    void kind;
    throw new EditorFeaturePendingError('补全触发', 'M8');
  }

  /** 文件内查找（作用于**内核当前文档**，避免 16ms 节流让查找用到旧值）。 */
  find(query: string, opts: FindOptions = {}): InFileMatch[] {
    if (!query) return [];
    return findInText(this.getValue(), query, opts);
  }

  /** 批量替换：单事务应用（撤销一步回退整批），后同步模型，避免整篇重灌。 */
  replace(matches: InFileMatch[], replacement: string): ReplaceResult {
    if (matches.length === 0) return { replaced: 0 };
    // setValue 还没回灌到内核时不能按新文本的偏移去 dispatch（视图里是旧文档）；这种情况只同步模型。
    const view = this.pendingExternal === null ? this.getView() : null;
    const { text, changes } = replaceInText(this.getValue(), matches, replacement);
    this.value = text;
    view?.dispatch({ changes });
    this.applyExternal?.(text);
    this.emit('change', text);
    return { replaced: matches.length };
  }

  revealLine(line: number, range?: { start: number; end: number }, options: { focus?: boolean } = {}): boolean {
    const view = this.getView();
    if (!view || !revealInView(view, line, range)) return false;
    // 默认把焦点放进编辑区（搜索/反链跳转后要能接着编辑）；查找面板传 focus:false 保住输入框焦点。
    if (options.focus !== false) this.focusEditor();
    return true;
  }

  focusEditor(): void {
    const view = this.getView();
    if (view) {
      view.focus();
      return;
    }
    // 视图尚未就绪（首帧）时退回内核内容 DOM；宿主是普通 div，focus() 对它无效（10.3 的根因）。
    this.host?.querySelector<HTMLElement>('.cm-content')?.focus();
  }

  destroy(): void {
    this.host = null;
    this.themeObserver?.disconnect();
    this.themeObserver = null;
    this.theme = null;
    this.editorRef.value = null;
    if (this.pendingTimer !== null) {
      clearTimeout(this.pendingTimer);
      this.pendingTimer = null;
    }
    this.pendingValue = null;
    this.pendingExternal = null;
    this.applyExternal = null;
    this.handlers.clear();
    this.app?.unmount();
    this.app = null;
  }

  /** 主题真相源是 `<html data-theme>`（settings-theme 负责写入并监听系统偏好）。 */
  private startThemeWatch(initial: ResolvedTheme | undefined): void {
    this.theme = ref<ResolvedTheme>(initial ?? currentTheme());
    this.themeObserver = new MutationObserver(() => {
      if (this.theme) this.theme.value = currentTheme();
    });
    this.themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  }

  private getView(): EditorViewLike | null {
    return editorViewOf(this.editorRef.value);
  }

  private emit(event: EditorEvent, ...args: unknown[]): void {
    for (const handler of this.handlers.get(event) ?? []) {
      handler(...args);
    }
  }

  /** 节流：立即发（距上次 ≥ 间隔）或安排尾部补发，保证最后一次变更不丢。 */
  private scheduleChange(next: string): void {
    if (this.suppressChange) return;
    this.pendingValue = next;
    const elapsed = Date.now() - this.lastEmitAt;
    if (elapsed >= CHANGE_THROTTLE_MS) {
      this.flushChange();
      return;
    }
    if (this.pendingTimer === null) {
      this.pendingTimer = setTimeout(() => {
        this.pendingTimer = null;
        this.flushChange();
      }, CHANGE_THROTTLE_MS - elapsed);
    }
  }

  private flushChange(): void {
    if (this.pendingTimer !== null) {
      clearTimeout(this.pendingTimer);
      this.pendingTimer = null;
    }
    if (this.pendingValue === null) return;
    const value = this.pendingValue;
    this.pendingValue = null;
    this.lastEmitAt = Date.now();
    this.emit('change', value);
  }
}
