import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import pluginVue from 'eslint-plugin-vue';
import vueParser from 'vue-eslint-parser';

export default tseslint.config(
  {
    ignores: [
      'dist/**', 'dist-*/**', 'node_modules/**', 'src-tauri/**', 'target/**', 'coverage/**', 'artifacts/**',
      // 工具脚本与 ESLint 自身配置不属于任何 tsconfig project，M0 暂不纳入类型化 lint
      'scripts/**', 'tests/**/*.mjs', 'eslint.config.js', 'probe/**',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  ...pluginVue.configs['flat/recommended'],
  {
    files: ['**/*.vue'],
    languageOptions: {
      parser: vueParser,
      parserOptions: { parser: tseslint.parser, project: './tsconfig.json', extraFileExtensions: ['.vue'] },
    },
  },
  {
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.json', './tsconfig.test.json', './tsconfig.node.json'],
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      'no-eval': 'error',
      'no-implied-eval': 'error',
      'no-new-func': 'error',
      'no-restricted-imports': ['error', { paths: [{ name: '@tauri-apps/api/core', message: '禁止直接调用 invoke()，请改用 src/core/ipc/client.ts 的类型安全封装' }] }],
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/no-explicit-any': 'error',
      'vue/multi-word-component-names': 'off',
      // 排版类规则交给 Prettier，避免与 --max-warnings 0 冲突
      'vue/max-attributes-per-line': 'off',
      'vue/singleline-html-element-content-newline': 'off',
      'vue/no-v-html': 'error',
      'no-restricted-properties': ['error',
        { object: 'DOMPurify', property: 'setConfig', message: 'R-05 红线：禁止使用 DOMPurify.setConfig()' },
        { object: 'DOMPurify', property: 'clearConfig', message: 'R-05 红线：禁止清除 DOMPurify 全局配置' },
      ],
    },
  },
  { files: ['src/core/ipc/**/*.ts'], rules: { 'no-restricted-imports': 'off' } },
  { files: ['src/core/markdown/**/*.vue', 'src/shared/components/KpSafeHtml.vue'], rules: { 'vue/no-v-html': 'off' } },
  {
    files: ['tests/**/*.ts', 'src/**/*.spec.ts'],
    ...tseslint.configs.disableTypeChecked,
  },
);
