// 单测环境：屏蔽真实 Tauri 运行时（前端不得在测试中触达 Rust）。
Object.defineProperty(globalThis, '__TAURI_INTERNALS__', {
  value: { invoke: () => Promise.reject(new Error('IPC disabled in unit tests')) },
  writable: true,
  configurable: true,
});
