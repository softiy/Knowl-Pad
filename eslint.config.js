import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import pluginVue from 'eslint-plugin-vue';
import vueParser from 'vue-eslint-parser';

export default tseslint.config(
  {
    ignores: [
      'dist/**', 'dist-*/**', 'node_modules/**', 'src-tauri/**', 'target/**', 'coverage/**', 'artifacts/**',
      // ESLint 自身配置不在任何 tsconfig project 里；scripts 见下方专用块（只挂长度规则，不走类型化）
      'tests/**/*.mjs', 'eslint.config.js', 'probe/**',
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
  // CODE-11 C11.4/C11.3：`scripts/**` 也纳入 lint —— 此前被全局 ignores 排除，于是 `main()` 长到 117 SLOC 无人报警。
  // 只挂**长度规则**：这些脚本不在任何 tsconfig project 里（故用 disableTypeChecked 关掉类型化规则），
  // 也不在这一步引入 20 个历史脚本的告警洪水 —— 全量 lint scripts 是独立议题，另行登记。
  // 实测：脚本现有函数在阈值 80 下 0 命中（阈值 10 对照实验 41 命中，证明探针有效）。
  {
    files: ['scripts/**/*.mjs', 'scripts/**/*.js'],
    ...tseslint.configs.disableTypeChecked,
    languageOptions: {
      // 关键：未被 files 限定的顶层块给所有文件设了 parserOptions.project，而 scripts 不在任何 tsconfig 里。
      // 显式关掉类型化解析（project/projectService = false），否则每个脚本都是 Parsing error。
      parserOptions: { project: false, projectService: false },
      globals: {
        process: 'readonly', console: 'readonly', Buffer: 'readonly', URL: 'readonly',
        URLSearchParams: 'readonly', FormData: 'readonly', Blob: 'readonly', AbortController: 'readonly',
        setTimeout: 'readonly', clearTimeout: 'readonly', structuredClone: 'readonly',
      },
    },
    rules: {
      // 注意：必须显式合并 disableTypeChecked 的 rules —— 直接再写一个 rules 键会**整体覆盖**它，
      // 于是类型化规则仍然开启、而脚本又不做类型检查 ⇒ "rule requires type information" 报错。
      ...tseslint.configs.disableTypeChecked.rules,
      'max-lines-per-function': ['error', { max: 80, skipBlankLines: true, skipComments: true }],
    },
  },
  // CODE-11 C11.3：**测试函数豁免**函数级长度约束（与文件级豁免同源；Rust 侧对应测试文件里的显式 allow）。
  { files: ['tests/**/*.ts', 'src/**/*.spec.ts'], rules: { 'max-lines-per-function': 'off' } },
  {
    files: ['tests/**/*.ts', 'src/**/*.spec.ts'],
    ...tseslint.configs.disableTypeChecked,
  },
);
