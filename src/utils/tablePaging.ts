/**
 * 项目页接口表的分页策略。
 *
 * 小项目维持「一屏看全」的现有习惯；超过阈值才自动分页，避免 500 行
 * 一次性渲染拖慢低端机。用户仍可手动「全部展开」。
 */

export const PROJECT_PAGE_THRESHOLD = 300
export const PROJECT_PAGE_SIZE = 50

export interface ProjectPagination {
  pageSize: number
  pageSizeOptions: string[]
  showSizeChanger: boolean
  showTotal: (total: number) => string
}

/**
 * `total` 不超过阈值或用户已选择全部展开时返回 `false`，
 * 直接交给 a-table 的 `pagination=false`。
 */
export function projectPagination(total: number, showAll = false): false | ProjectPagination {
  if (showAll || total <= PROJECT_PAGE_THRESHOLD) return false
  return {
    pageSize: PROJECT_PAGE_SIZE,
    pageSizeOptions: ['50', '100', '200'],
    showSizeChanger: true,
    showTotal: (count) => `共 ${count} 个接口`,
  }
}

/** 把行下标换算成 1 起算的页码；用于定位「上次打开的接口」所在页。 */
export function pageOfIndex(index: number, pageSize: number): number {
  if (index <= 0) return 1
  return Math.floor(index / pageSize) + 1
}
