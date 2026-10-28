/**
 * 把"当前缓冲区内容"同步进编辑内核（AC-EDITOR-06 的两条边界）。
 *
 * 为什么只监听 `relPath` 与 `replacedAt`、**不监听 content**：内核自身产生的编辑也会改变 content，
 * 回灌会清空内核的撤销栈。抽成组合式是为了让 EditorView 的 setup 留在 CODE-11 的函数行数线内。
 */
import { watch } from 'vue';

export function useEditorBufferSync(opts: {
  relPath: () => string | null;
  replacedAt: () => number | undefined;
  content: () => string;
  adapter: () => { setValue(value: string): void } | null;
}): void {
  const push = (): void => {
    opts.adapter()?.setValue(opts.content());
  };
  watch(opts.relPath, push);
  watch(opts.replacedAt, push);
}
