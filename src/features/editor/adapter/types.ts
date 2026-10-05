/** 编辑器内核适配层的公共契约（技术方案 §2.2 `KpEditorAdapter`；实现约束见 §7.4 ED-01~ED-06）。
 *
 * 其余模块**只依赖本接口**，M8 换内核（md-editor-v3 → CodeMirror 6）不外溢。
 */

export interface EditorOptions {
  initialValue?: string;
  readonly?: boolean;
  theme?: 'light' | 'dark';
  placeholder?: string;
}

export interface FindOptions {
  caseSensitive?: boolean;
  wholeWord?: boolean;
  regex?: boolean;
}

/** 文件内匹配（偏移为 UTF-16 码元下标）。 */
export interface InFileMatch {
  from: number;
  to: number;
  text: string;
}

export interface ReplaceResult {
  replaced: number;
}

export interface CursorPosition {
  line: number;
  column: number;
}

export type EditorEvent = 'change' | 'cursor' | 'save';

export interface KpEditorAdapter {
  /** 挂载到给定宿主元素；重复挂载应复用同一实例（ED-01）。 */
  mount(el: HTMLElement, opts: EditorOptions): void;
  getValue(): string;
  /** 程序化写入**不得**触发 change（否则打开笔记即变脏）。 */
  setValue(content: string): void;
  on(event: EditorEvent, handler: (...args: unknown[]) => void): void;
  insertLink(target: string, alias?: string): void;
  triggerSuggest(kind: 'link' | 'tag' | 'heading'): void;
  find(query: string, opts: FindOptions): InFileMatch[];
  replace(matches: InFileMatch[], replacement: string): ReplaceResult;
  /** 释放全部资源（DOM 监听器、定时器、编辑器实例）——ED-01。 */
  destroy(): void;
}

/** 尚未接入的功能（显式失败，绝不静默 no-op）。 */
export class EditorFeaturePendingError extends Error {
  constructor(feature: string, milestone: string) {
    super(`${feature} 尚未接入：需要索引/补全数据，计划在 ${milestone} 实现`);
    this.name = 'EditorFeaturePendingError';
  }
}
