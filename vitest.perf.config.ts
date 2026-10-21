import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';
import { fileURLToPath } from 'node:url';

/**
 * **门禁 10 专用配置**（DEBT-31）：只收集 `tests/perf/**`，环境与主配置一致（jsdom + 同一套路径别名 + vue 插件）。
 *
 * 为什么需要它：主配置把 `tests/perf/**` 排除在 `pnpm test` 之外（性能测量不该进日常单测，否则既慢又抖），
 * 于是性能探针必须由 `scripts/run-perf.mjs` 用**自己的配置**显式拉起。
 *
 * 为什么不用 `mergeConfig(base, …)`：Vite 的合并对**数组是合并而非覆盖** —— 主配置的
 * `exclude: ['tests/perf/**', …]` 会被并进来，结果命令"成功"跑完却只跑了单测（实测踩过）。
 * 这里显式复制需要保持同步的两项（别名、vue 插件），来源是 `vitest.config.ts`。
 */
const alias = {
  '@': fileURLToPath(new URL('./src', import.meta.url)),
  '@core': fileURLToPath(new URL('./src/core', import.meta.url)),
  '@features': fileURLToPath(new URL('./src/features', import.meta.url)),
  '@shared': fileURLToPath(new URL('./src/shared', import.meta.url)),
};

export default defineConfig({
  plugins: [vue()],
  resolve: { alias },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['tests/perf/**/*.spec.ts'],
    exclude: ['node_modules/**'],
    setupFiles: ['tests/unit/setup.ts'],
  },
});