import { beforeEach, describe, expect, it } from 'vitest'
import {
  countEdits,
  emptyOverlay,
  extensionFor,
  loadOverlay,
  overlayKey,
  sanitizeFileName,
  saveOverlay,
} from '@/utils/docsExport'

function memoryStorage() {
  const map = new Map<string, string>()
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
    clear: () => map.clear(),
    key: (index: number) => [...map.keys()][index] ?? null,
    get length() {
      return map.size
    },
  } as Storage
}

beforeEach(() => {
  Object.defineProperty(globalThis, 'localStorage', {
    value: memoryStorage(),
    configurable: true,
  })
})

describe('docsExport', () => {
  it('净化文件名并保留中文', () => {
    expect(sanitizeFileName('开放平台 / 登录: v2?')).toBe('开放平台 _ 登录_ v2_')
    expect(sanitizeFileName('   ')).toBe('接口文档')
    expect(sanitizeFileName('a'.repeat(200)).length).toBe(120)
  })

  it('格式扩展名映射', () => {
    expect(extensionFor('markdown')).toBe('.md')
    expect(extensionFor('html')).toBe('.html')
  })

  it('覆盖层按 scope + targetId + environmentId 隔离', () => {
    const overlay = emptyOverlay('rev1')
    overlay.meta.title = '标题'
    saveOverlay('project', 1, 2, overlay)
    expect(loadOverlay('project', 1, 2)?.meta.title).toBe('标题')
    expect(loadOverlay('project', 1, 3)).toBeNull()
    expect(overlayKey('project', 1, 2)).toBe('astroman:docs-overlay:project:1:2')
  })

  it('统计编辑数量', () => {
    const overlay = emptyOverlay('rev1')
    expect(countEdits(overlay)).toBe(0)
    overlay.meta.title = '标题'
    overlay.nodes['r-1'] = { description: '说明', exampleValues: { 'p-1-query-a-1': 'x' } }
    overlay.excluded = ['r-2']
    expect(countEdits(overlay)).toBe(4)
  })

  it('超过 1 MB 不写 localStorage 但不清空内存对象', () => {
    const overlay = emptyOverlay('rev1')
    overlay.nodes['r-1'] = {
      description: 'x',
      exampleValues: { k: 'y'.repeat(1024 * 1024 + 1) },
    }
    saveOverlay('project', 1, 2, overlay)
    expect(loadOverlay('project', 1, 2)).toBeNull()
    expect(overlay.nodes['r-1'].exampleValues.k.length).toBeGreaterThan(1024 * 1024)
  })
})
