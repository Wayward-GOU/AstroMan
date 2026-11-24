import { computed, ref } from 'vue'

export type SplitDirection = 'horizontal' | 'vertical'

/**
 * 工具箱分栏的方向与比例。
 *
 * 为什么比例要按方向分开存：左右各占 50% 宽和上下各占 50% 高是两种完全不同的
 * 视觉结果。共用一个 key 会出现「上下布局调好高度，切回左右后宽度全乱」。
 */
export function useSplitLayout(
  storageKey: string,
  defaultDirection: SplitDirection = 'vertical',
) {
  const dirKey = `${storageKey}:dir`

  function readDirection(): SplitDirection {
    try {
      const raw = localStorage.getItem(dirKey)
      if (raw === 'vertical' || raw === 'horizontal') return raw
    } catch {
      // localStorage 不可用（隐私模式）时退回默认方向
    }
    return defaultDirection
  }

  const direction = ref<SplitDirection>(readDirection())

  function setDirection(next: SplitDirection) {
    if (direction.value === next) return
    direction.value = next
    try {
      localStorage.setItem(dirKey, next)
    } catch {
      // 写入失败只影响下次启动的默认值，当前会话照常
    }
  }

  function toggleDirection() {
    setDirection(direction.value === 'horizontal' ? 'vertical' : 'horizontal')
  }

  const isVertical = computed(() => direction.value === 'vertical')

  /** 传给 SplitPane：每个方向各自记住自己的比例 */
  const ratioKey = computed(() => `${storageKey}:ratio:${direction.value}`)

  return { direction, isVertical, setDirection, toggleDirection, ratioKey }
}
