import type { WatchStopHandle } from 'vue';
import { watch } from 'vue';

export interface RevealRequest {
  relPath: string;
  line: number;
}

export interface ConsumeRevealOptions {
  current: () => RevealRequest | null;
  active: () => string | null;
  clear: () => void;
  adapter: () => { revealLine(line: number): boolean } | null;
  /** 适配器不具备定位能力时回调（调用方**如实降级**，不假装定位过）。 */
  onDegrade?: () => void;
}

/**
 * 消费"待定位行"（FR-LINK-13）。
 *
 * 单独成文件：EditorView 的模板块已经很密，再挂十几行逻辑会越过 CODE-11 的 200 行线。
 * 契约刻意用**回调**而非 store 对象：不依赖 Pinia 的 store 形状，只依赖三个动作，测试也好写。
 *
 * 语义：打开笔记是**异步**的，所以请求记在 store 里；这里每次变化只消费一次，
 * 且**只有**当活动笔记就是请求的那篇时才消费（否则等下一次）。
 */
export function consumeReveal(opts: ConsumeRevealOptions): WatchStopHandle {
  return watch(
    () => opts.current(),
    (req) => {
      if (!req) return;
      const adapter = opts.adapter();
      if (!adapter) return;
      if (opts.active() !== req.relPath) return;
      if (!adapter.revealLine(req.line)) {
        // 默认降级：只提示，不假装定位过
        (opts.onDegrade ?? (() => console.warn('编辑器未提供行定位能力，已降级为仅打开笔记')))();
      }
      opts.clear();
    },
    { immediate: true },
  );
}
