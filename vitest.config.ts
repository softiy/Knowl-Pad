import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';
import { fileURLToPath } from 'node:url';

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      '@core': fileURLToPath(new URL('./src/core', import.meta.url)),
      '@features': fileURLToPath(new URL('./src/features', import.meta.url)),
      '@shared': fileURLToPath(new URL('./src/shared', import.meta.url)),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['tests/unit/**/*.spec.ts', 'tests/unit/**/*.spec.mjs', 'tests/security/**/*.spec.ts', 'src/**/*.spec.ts'],
    exclude: ['tests/perf/**', 'tests/e2e/**', 'node_modules/**'],
    setupFiles: ['tests/unit/setup.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,vue}'],
      reporter: ['text','json-summary'],
      thresholds: {
        'src/core/utils/**': { lines: 90, functions: 90, branches: 85, statements: 90 },
        // DEBT-21：此前只有全局阈值，单模块可被平均值掩盖。
        'src/app/router/**': { lines: 65, functions: 45, branches: 45, statements: 65 },
        // features 层（应用主体，20 文件）实测 93.65/84.20/77.78 → 阈值按实测 −3 锁定，只许升不许降。
        'src/features/**': { lines: 90, functions: 74, branches: 81, statements: 90 },
        lines: 80, functions: 80, branches: 75, statements: 80,
      },
      exclude: ['**/*.d.ts','src/main.ts','src/core/ipc/**','tests/**'],
    },
  },
});
