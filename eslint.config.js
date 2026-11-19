import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'

/**
 * 前端静态门禁（R4 / A6）
 *
 * 只做「正确性」规则，不做格式统一：
 *   - 类型错误归 `vue-tsc --noEmit`；
 *   - 未使用变量、浮动 Promise、v-for 缺 key、直接改 props 这类问题归 ESLint；
 *   - 不引入 Prettier，避免一次性重排 22 个视图、淹没 review。
 */
export default defineConfigWithVueTs(
  {
    name: 'astroman/ignores',
    ignores: [
      'dist/**',
      'node_modules/**',
      'src-tauri/**',
      'test-results/**',
      'playwright-report/**',
      'coverage/**',
      'scripts/**',
      'e2e/**',
      'evals/**',
      '*.config.js',
      '*.config.ts',
      '*.timestamp-*.mjs',
      'vitest.eval.config.ts',
    ],
  },
  // 只挂 Vue 解析器；规则用下面显式列出的正确性项，不整包启用 essential
  pluginVue.configs['flat/base'],
  vueTsConfigs.recommended,
  {
    name: 'astroman/rules',
    languageOptions: {
      parserOptions: {
        // 仅 no-floating-promises / no-misused-promises 需要类型信息
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: ['.vue'],
      },
    },
    rules: {
      // 正确性，直接阻断
      'no-unused-vars': 'off',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      'vue/no-mutating-props': 'error',
      'vue/require-v-for-key': 'error',
      'vue/no-use-v-if-with-v-for': 'error',
      'vue/valid-v-model': 'error',
      // 视图名就是单个单词（Reports / Settings），这条只约束组件命名风格
      'vue/multi-word-component-names': 'off',
      // 可疑但先不阻断
      'vue/no-unused-components': 'warn',
      '@typescript-eslint/no-explicit-any': 'warn',
    },
  },
)
