/**
 * 界面缩放的口径与算术。
 *
 * 与 Postman / Apifox 保持一致：范围 50%~200%，每次 10%，预设档位一口气可选。
 * 这类工具都不在界面上常驻加减按钮——缩放属于「偶尔调一次」的设置，
 * 常驻控件只会占用导航区，所以入口放在设置页、命令面板与快捷键上。
 */

export const ZOOM_MIN = 0.5
export const ZOOM_MAX = 2
export const ZOOM_STEP = 0.1

/** 预设档位：覆盖「笔记本看不清」「投影演示」两类常见需求 */
export const ZOOM_PRESETS = [50, 75, 90, 100, 110, 125, 150, 200] as const

/** 收敛到 50%~200%，并保留两位小数，避免 1.1000000000000001 这类值 */
export function clampZoom(value: number): number {
  if (!Number.isFinite(value)) return 1
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(value * 100) / 100))
}

/**
 * 按一次步长缩放。
 * 用整数百分比累加而不是 `current + 0.1`：浮点累加会得到 109.99999%，
 * 界面上就会显示 110% 却与 125% 的预设对不上。
 */
export function stepZoom(current: number, direction: 1 | -1): number {
  const percent = Math.round(clampZoom(current) * 100) + direction * Math.round(ZOOM_STEP * 100)
  return clampZoom(percent / 100)
}

export function toPercent(zoom: number): number {
  return Math.round(clampZoom(zoom) * 100)
}

export function fromPercent(percent: number): number {
  return clampZoom(percent / 100)
}
