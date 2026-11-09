import type { DocEditOverlay, DocFormat, DocScope } from '@/types/docs'

const MAX_OVERLAY_BYTES = 1024 * 1024
const STORAGE_PREFIX = 'astroman:docs-overlay'

export function sanitizeFileName(input: string, fallback = '接口文档'): string {
  const cleaned = input
    .replace(/[\\/:*?"<>|]/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
  return (cleaned || fallback).slice(0, 120)
}

export function extensionFor(format: DocFormat): string {
  return format === 'markdown' ? '.md' : '.html'
}

export function overlayKey(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
): string {
  return `${STORAGE_PREFIX}:${scope}:${targetId}:${environmentId ?? 'default'}`
}

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage
  } catch {
    return null
  }
}

export function emptyOverlay(revision = ''): DocEditOverlay {
  return { revision, meta: {}, nodes: {}, excluded: [], order: {} }
}

export function loadOverlay(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
): DocEditOverlay | null {
  const store = storage()
  if (!store) return null
  try {
    const raw = store.getItem(overlayKey(scope, targetId, environmentId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<DocEditOverlay>
    return {
      revision: parsed.revision ?? '',
      meta: parsed.meta ?? {},
      nodes: parsed.nodes ?? {},
      excluded: parsed.excluded ?? [],
      order: parsed.order ?? {},
    }
  } catch {
    return null
  }
}

/** 超过 1 MB 时只跳过 localStorage 写入，内存中的覆盖层仍可导出 */
export function saveOverlay(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
  overlay: DocEditOverlay,
): void {
  const store = storage()
  if (!store) return
  const raw = JSON.stringify(overlay)
  if (new TextEncoder().encode(raw).length > MAX_OVERLAY_BYTES) return
  try {
    store.setItem(overlayKey(scope, targetId, environmentId), raw)
  } catch {
    // 配额满 / 隐私模式：退化为内存暂存，不阻断导出
  }
}

export function clearOverlay(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
): void {
  const store = storage()
  try {
    store?.removeItem(overlayKey(scope, targetId, environmentId))
  } catch {
    // 忽略
  }
}

export function countEdits(overlay: DocEditOverlay): number {
  let count = 0
  if (overlay.meta.title !== undefined) count += 1
  if (overlay.meta.subtitle !== undefined) count += 1
  for (const node of Object.values(overlay.nodes)) {
    if (node.description !== undefined) count += 1
    count += Object.keys(node.exampleValues ?? {}).length
  }
  count += overlay.excluded.length
  count += Object.values(overlay.order).filter((keys) => keys.length > 0).length
  return count
}

export function debounce<T extends (...args: never[]) => void>(fn: T, wait = 300): T {
  let timer: ReturnType<typeof setTimeout> | null = null
  return ((...args: never[]) => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => fn(...args), wait)
  }) as T
}
