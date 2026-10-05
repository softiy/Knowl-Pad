/// <reference types="vite/client" />
declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<Record<string, unknown>, Record<string, unknown>, unknown>;
  export default component;
}

/** 构建时环境变量（ED-05：编辑器内核选择）。 */
interface ImportMetaEnv {
  /** 'md-editor-v3'（默认）| 'codemirror6'（M8 起） */
  readonly VITE_EDITOR_ENGINE?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

/** 样式副作用导入（Vite 负责打包，TS 只需知道它存在）。 */
declare module '*.css';
