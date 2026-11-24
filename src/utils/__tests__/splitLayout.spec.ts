import { beforeEach, describe, expect, it } from 'vitest'
import { useSplitLayout } from '@/composables/useSplitLayout'

/** node 环境没有 localStorage，装一个最小替身 */
const bag = new Map<string, string>()

beforeEach(() => {
  bag.clear()
  Object.defineProperty(globalThis, 'localStorage', {
    value: {
      getItem: (key: string) => bag.get(key) ?? null,
      setItem: (key: string, value: string) => void bag.set(key, value),
      removeItem: (key: string) => void bag.delete(key),
      clear: () => bag.clear(),
    },
    configurable: true,
  })
})

describe('useSplitLayout', () => {
  it('默认上下分栏，比例键带方向后缀', () => {
    const { direction, ratioKey } = useSplitLayout('astroman:tools:json')
    expect(direction.value).toBe('vertical')
    expect(ratioKey.value).toBe('astroman:tools:json:ratio:vertical')
  })

  it('切换方向会落库，比例键同步更换', () => {
    const { setDirection, ratioKey } = useSplitLayout('astroman:tools:json')
    setDirection('horizontal')
    expect(bag.get('astroman:tools:json:dir')).toBe('horizontal')
    expect(ratioKey.value).toBe('astroman:tools:json:ratio:horizontal')
  })

  it('重启后沿用上次选择的方向', () => {
    bag.set('astroman:tools:json:dir', 'horizontal')
    const { direction } = useSplitLayout('astroman:tools:json')
    expect(direction.value).toBe('horizontal')
  })

  it('存储里是非法值时退回默认方向', () => {
    bag.set('astroman:tools:json:dir', 'diagonal')
    const { direction } = useSplitLayout('astroman:tools:json')
    expect(direction.value).toBe('vertical')
  })

  it('两个方向的键互不覆盖，切换不会互相污染', () => {
    const a = useSplitLayout('k')
    expect(a.ratioKey.value).toBe('k:ratio:vertical')
    a.setDirection('horizontal')
    expect(a.ratioKey.value).toBe('k:ratio:horizontal')
    a.setDirection('vertical')
    expect(a.ratioKey.value).toBe('k:ratio:vertical')
  })
})
