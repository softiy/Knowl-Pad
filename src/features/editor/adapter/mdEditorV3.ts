import { revealLineIn } from './revealLine';
import { createApp, defineComponent, h, ref, watch, type App } from 'vue';
import { MdEditor } from 'md-editor-v3';
import 'md-editor-v3/lib/style.css';
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
 * MVP 内核适配器（md-editor-v3 6.x）。
 *
 * - **ED-01**：`destroy()` 清定时器 + 卸载 Vue 实例 + 清处理器；
 * - **ED-02**：`change` 走 16ms 节流（尾部补发最后一次，保证不丢内容）；
 * - **ED-04**：不在这里绑定 keydown，快捷键由 `core/shortcut` 统一处理；
 * - **ED-06**：代码高亮用 md-editor-v3 内置，不额外引入高亮库。
 */
export class MdEditorV3Adapter implements KpEditorAdapter {
  private app: App<Element> | null = null;
  /** 宿主元素（mount 时记下；revealLine 靠它找到可编辑元素）。 */
  private host: HTMLElement | null = null;
  private value = '';
  private readonly handlers = new Map<EditorEvent, Set<(...args: unknown[]) => void>>();
  private applyExternal: ((next: string) => void) | null = null;
  private pendingValue: string | null = null;
  private pendingTimer: ReturnType<typeof setTimeout> | null = null;
  private lastEmitAt = 0;
  /** 程序化写入期间抑制 change（避免打开笔记就变脏）。 */
  private suppressChange = false;

  mount(el: HTMLElement, opts: EditorOptions): void {
    this.host = el;
    if (this.app) {
      return; // 已挂载：复用实例（ED-01）
    }
    this.value = opts.initialValue ?? '';
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
            modelValue: text.value,
            'onUpdate:modelValue': (next: string) => {
              text.value = next;
            },
            preview: false, // 阅读/分屏的预览走 §7.2 的净化管线，不用内核自带预览
            theme: opts.theme ?? 'light',
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
    this.flushChange();
    return this.value;
  }

  setValue(content: string): void {
    this.suppressChange = true;
    this.value = content;
    this.applyExternal?.(content);
    this.suppressChange = false;
  }

  on(event: EditorEvent, handler: (...args: unknown[]) => void): void {
    const set = this.handlers.get(event) ?? new Set();
    set.add(handler);
    this.handlers.set(event, set);
  }

  insertLink(target: string, alias?: string): void {
    // 链接补全的插入需要索引与光标事务（ED-03 的补全流程），M4 接入；显式失败而非静默无效
    // 链接补全的插入需要索引与光标事务（ED-03 的补全流程在适配器之外），M4 接入；
    // 这里显式失败而非静默无效——参数仅为满足契约签名。
    void target;
    void alias;
    throw new EditorFeaturePendingError('链接补全与插入', 'M4');
  }

  triggerSuggest(kind: 'link' | 'tag' | 'heading'): void {
    void kind;
    throw new EditorFeaturePendingError('补全触发', 'M4');
  }

  /** 文件内查找（纯函数式，作用于当前缓冲区内容）。 */
  find(query: string, opts: FindOptions = {}): InFileMatch[] {
    if (!query) return [];
    const text = this.getValue();
    const flags = opts.caseSensitive ? 'g' : 'gi';
    const pattern = opts.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const source = opts.wholeWord ? `\\b(?:${pattern})\\b` : pattern;
    const matches: InFileMatch[] = [];
    const re = new RegExp(source, flags);
    for (const m of text.matchAll(re)) {
      const from = m.index ?? 0;
      matches.push({ from, to: from + m[0].length, text: m[0] });
    }
    return matches;
  }

  /** 批量替换（从后往前，避免偏移失效）。 */
  replace(matches: InFileMatch[], replacement: string): ReplaceResult {
    if (matches.length === 0) return { replaced: 0 };
    let text = this.getValue();
    const ordered = [...matches].sort((a, b) => b.from - a.from);
    for (const match of ordered) {
      text = text.slice(0, match.from) + replacement + text.slice(match.to);
    }
    this.setValue(text);
    this.emit('change', text);
    return { replaced: matches.length };
  }

  revealLine(line: number): boolean {
    return revealLineIn(this.host, this.value, line);
  }

  destroy(): void {
    this.host = null;
    if (this.pendingTimer !== null) {
      clearTimeout(this.pendingTimer);
      this.pendingTimer = null;
    }
    this.pendingValue = null;
    this.applyExternal = null;
    this.handlers.clear();
    this.app?.unmount();
    this.app = null;
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
