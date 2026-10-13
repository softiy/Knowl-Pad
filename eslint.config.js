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
      // CODE-11 C11.3：函数级上限（口径与 CODE-11 的 SLOC 一致 —— 跳过空行与纯注释行）
      'max-lines-per-function': ['error', { max: 80, skipBlankLines: true, skipComments: true }],
      'no-restricted-properties': ['error',
        { object: 'DOMPurify', property: 'setConfig', message: 'R-05 红线：禁止使用 DOMPurify.setConfig()' },
        { object: 'DOMPurify', property: 'clearConfig', message: 'R-05 红线：禁止清除 DOMPurify 全局配置' },
      ],
    },
  },
  // Pinia 的 setup store 是「容器」而不是「巨型函数」：C11.3 给它单独上限 220（当前最大 208）。
  // 记录理由而不是关掉规则：这些 store 另登记为 DEBT-36 逐步拆分。
  { files: ['src/**/stores/*.ts'], rules: { 'max-lines-per-function': ['error', { max: 220, skipBlankLines: true, skipComments: true }] } },
  { files: ['src/core/ipc/**/*.ts'], rules: { 'no-restricted-imports': 'off' } },
  { files: ['src/core/markdown/**/*.vue', 'src/shared/components/KpSafeHtml.vue'], rules: { 'vue/no-v-html': 'off' } },
  // CODE-11 C11.3：**测试函数豁免**函数级长度约束（与文件级豁免同源；Rust 侧对应测试文件里的显式 allow）。
  { files: ['tests/**/*.ts', 'src/**/*.spec.ts'], rules: { 'max-lines-per-function': 'off' } },
  {
    files: ['tests/**/*.ts', 'src/**/*.spec.ts'],
    ...tseslint.configs.disableTypeChecked,
  },
);
