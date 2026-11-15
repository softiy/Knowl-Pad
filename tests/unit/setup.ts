// 单测环境：屏蔽真实 Tauri 运行时（前端不得在测试中触达 Rust）。
Object.defineProperty(globalThis, '__TAURI_INTERNALS__', {
  value: { invoke: () => Promise.reject(new Error('IPC disabled in unit tests')) },
  writable: true,
  configurable: true,
});

// jsdom 不实现布局：@codemirror/view 的选区绘制（drawSelection）会在 requestAnimationFrame 里
// 调用 Range.getClientRects() / getBoundingClientRect()，而 jsdom 的 Range 两者都没有 ——
// 一旦测试挂载**真实**编辑器并产生选区（查找定位/反链跳转），就会抛 TypeError 并成为
// vitest 的 unhandled error（整个 spec 文件 exit 1，尽管断言全过）。真实 WebView 有实现，
// 这里只补"零几何"：measure 得到空矩形，不影响任何断言。
const EMPTY_RECT = {
  x: 0,
  y: 0,
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
  width: 0,
  height: 0,
  toJSON: () => ({}),
} as DOMRect;

if (typeof Range !== 'undefined') {
  if (typeof Range.prototype.getClientRects !== 'function') {
    Range.prototype.getClientRects = () =>
      Object.assign([], { item: () => null }) as unknown as DOMRectList;
  }
  if (typeof Range.prototype.getBoundingClientRect !== 'function') {
    Range.prototype.getBoundingClientRect = () => EMPTY_RECT;
  }
}
