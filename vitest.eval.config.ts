import { defineConfig } from 'vitest/config'
import { fileURLToPath, URL } from 'node:url'

// AI-10 评测专用配置：默认 `npm test` 只跑 src/**/*.spec.ts，不会触发真实模型调用
export default defineConfig({
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
    include: ['evals/**/*.spec.ts'],
    testTimeout: 600_000,
    hookTimeout: 60_000,
    fileParallelism: false,
  },
})
