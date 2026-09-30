import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { resolve } from 'node:path';

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, 'src'),
      '@core': resolve(import.meta.dirname, 'src/core'),
      '@features': resolve(import.meta.dirname, 'src/features'),
      '@shared': resolve(import.meta.dirname, 'src/shared'),
    },
  },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: {
    target: 'es2024',
    sourcemap: false,
    minify: 'oxc',
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: 'vendor-vue', test: /node_modules[\\/](vue|@vue|pinia|vue-router)[\\/]/ },
            { name: 'vendor-md', test: /node_modules[\\/](markdown-it|dompurify)[\\/]/ },
          ],
        },
      },
    },
  },
  envPrefix: 'VITE_',
});
