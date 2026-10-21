/**
 * 最小 HTML 转义（技术方案 §2.2 的 **HL-02**）：`&`、`<`、`>`、`"`、`'` **全部**转义。
 *
 * 为什么单独一个文件：高亮扫描（`highlightCode.ts` / `highlightMarkdown.ts`）与公共 API（`highlight.ts`）
 * 都要用它；放在任一实现里会形成循环依赖。
 */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}