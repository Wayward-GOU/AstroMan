import { webcrypto } from 'node:crypto'

// Vitest 的 node 环境运行在独立上下文里，Node 18 不会暴露全局 crypto；
// 造数、TOTP、JWT 验签等工具依赖 WebCrypto，这里补上。
if (typeof globalThis.crypto === 'undefined') {
  Object.defineProperty(globalThis, 'crypto', { value: webcrypto, configurable: true })
}
