import { describe, expect, it } from 'vitest'
import {
  ZOOM_MAX,
  ZOOM_MIN,
  ZOOM_PRESETS,
  clampZoom,
  fromPercent,
  stepZoom,
  toPercent,
} from '@/utils/zoom'

describe('clampZoom', () => {
  it('收敛到 50% ~ 200%', () => {
    expect(clampZoom(0.1)).toBe(ZOOM_MIN)
    expect(clampZoom(5)).toBe(ZOOM_MAX)
    expect(clampZoom(1.25)).toBe(1.25)
  })

  it('非法值回到 100%', () => {
    expect(clampZoom(Number.NaN)).toBe(1)
    expect(clampZoom(Number.POSITIVE_INFINITY)).toBe(1)
  })

  it('去掉浮点尾巴', () => {
    expect(clampZoom(1.1000000000000001)).toBe(1.1)
  })
})

describe('stepZoom', () => {
  it('按 10% 步进且不出现浮点误差', () => {
    let value = 1
    for (let i = 0; i < 4; i += 1) value = stepZoom(value, 1)
    expect(value).toBe(1.4)
    expect(toPercent(value)).toBe(140)
  })

  it('触到上下限就不再变化', () => {
    expect(stepZoom(ZOOM_MAX, 1)).toBe(ZOOM_MAX)
    expect(stepZoom(ZOOM_MIN, -1)).toBe(ZOOM_MIN)
  })

  it('从预设档位出发仍然对齐整数百分比', () => {
    expect(toPercent(stepZoom(1.25, 1))).toBe(135)
    expect(toPercent(stepZoom(1.25, -1))).toBe(115)
  })
})

describe('百分比换算', () => {
  it('toPercent / fromPercent 互逆', () => {
    for (const preset of ZOOM_PRESETS) {
      expect(toPercent(fromPercent(preset))).toBe(preset)
    }
  })

  it('预设档位都在允许范围内', () => {
    for (const preset of ZOOM_PRESETS) {
      expect(preset).toBeGreaterThanOrEqual(ZOOM_MIN * 100)
      expect(preset).toBeLessThanOrEqual(ZOOM_MAX * 100)
    }
  })
})
