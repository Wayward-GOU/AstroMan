/**
 * 请求体体积口径。
 *
 * 为什么单独定上限：body 要经过 Tauri IPC（JSON 序列化）、写入 SQLite、再交给 reqwest。
 * 这三步的开销都只取决于本机，和服务端愿意接收多大正文无关。十几 MB 的正文会让
 * 自动保存与发送各卡一次，所以在前端先拦，Rust 侧再兜底。
 *
 * 阈值取值依据：
 * - 4MB：CodeMirror 打开 5MB 正文实测 62ms 长任务，再往上体感变明显，作为「警戒」线
 * - 32MB：编辑器仍能打开（16MB 实测 139ms），但 IPC + 落库已经不值得再做，作为硬线
 */

/** 建议上限：超过后体积提示转为警戒色，说明保存与发送会变慢 */
export const REQUEST_BODY_WARN_BYTES = 4 * 1024 * 1024

/** 硬上限：超过后禁止发送。必须与 Rust 侧 `http::execute_with` 的校验保持一致 */
export const REQUEST_BODY_MAX_BYTES = 32 * 1024 * 1024

/**
 * 单个上传文件的上限：2 GB。必须与 Rust 侧 `http::MAX_UPLOAD_FILE_BYTES` 保持一致。
 * 文件由后端整份读进内存发送，这个上限是为了不让一次误选把进程顶爆。
 */
export const UPLOAD_FILE_MAX_BYTES = 2 * 1024 * 1024 * 1024

export type BodySizeLevel = 'ok' | 'warn' | 'blocked'

/**
 * 正文的 UTF-8 字节数。
 *
 * 用 Blob 而不是 `text.length`：中文一个字符是 3 字节，按字符数判断会把 11MB 的
 * 中文正文当成 3.7MB 放过去。fallback 给非浏览器环境（单测）用 TextEncoder。
 */
export function bodyBytes(text: string | null | undefined): number {
  if (!text) return 0
  if (typeof Blob !== 'undefined') return new Blob([text]).size
  return new TextEncoder().encode(text).length
}

export function bodySizeLevel(bytes: number): BodySizeLevel {
  if (bytes > REQUEST_BODY_MAX_BYTES) return 'blocked'
  if (bytes > REQUEST_BODY_WARN_BYTES) return 'warn'
  return 'ok'
}

/**
 * 正文里只存文件路径的 Body 类型：文件内容由 Rust 侧从本机读取，
 * 既不经过 IPC 也不写进 SQLite，所以不受上面两个阈值约束。
 * 与 Rust 侧 `text_body_bytes` 的口径保持一致。
 */
export function isFileBackedBodyType(bodyType: string | null | undefined): boolean {
  return bodyType === 'binary' || bodyType === 'multipart'
}
