import { MdEditorV3Adapter } from './mdEditorV3';
import type { KpEditorAdapter } from './types';

export type EditorEngine = 'md-editor-v3' | 'codemirror6';

/**
 * 构建时内核选择（ED-05：MVP 的 md-editor-v3 与正式版 CodeMirror 6 **不共存于同一构建**）。
 * 取值来自 `VITE_EDITOR_ENGINE`，未设置时用 MVP 内核。
 */
export function currentEngine(): EditorEngine {
  const raw = (import.meta.env.VITE_EDITOR_ENGINE ?? 'md-editor-v3').toString().toLowerCase();
  return raw === 'codemirror6' ? 'codemirror6' : 'md-editor-v3';
}

/**
 * 创建当前内核的适配器。
 *
 * M8 接入 CodeMirror 6 时，这里改成对 `import.meta.env.VITE_EDITOR_ENGINE` 的**静态可判定**分支
 * （`await import(...)`），由 Vite 在构建期消除另一支，从而保证包内只有一套内核。
 * 在该模块存在之前，选择 codemirror6 会**显式失败**，而不是悄悄回退到 MVP 内核。
 */
export function createEditorAdapter(): KpEditorAdapter {
  if (currentEngine() === 'codemirror6') {
    throw new Error('CodeMirror 6 内核计划在 M8 接入（ED-05：两套内核不共存于同一构建）');
  }
  return new MdEditorV3Adapter();
}

export * from './types';
