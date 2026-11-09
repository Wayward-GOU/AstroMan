import { describe, expect, it } from 'vitest'
import {
  PROJECT_PAGE_SIZE,
  PROJECT_PAGE_THRESHOLD,
  pageOfIndex,
  projectPagination,
} from '../tablePaging'

describe('projectPagination', () => {
  it('小项目不分页，保持一屏看全', () => {
    expect(projectPagination(0)).toBe(false)
    expect(projectPagination(PROJECT_PAGE_THRESHOLD)).toBe(false)
  })

  it('超过阈值自动分页 50 条', () => {
    const config = projectPagination(PROJECT_PAGE_THRESHOLD + 1)
    expect(config).not.toBe(false)
    if (config === false) return
    expect(config.pageSize).toBe(PROJECT_PAGE_SIZE)
    expect(config.showSizeChanger).toBe(true)
  })

  it('用户手动全部展开后不再分页', () => {
    expect(projectPagination(PROJECT_PAGE_THRESHOLD + 1, true)).toBe(false)
  })
})

describe('pageOfIndex', () => {
  it('按 1 起算并落在正确的页', () => {
    expect(pageOfIndex(0, 50)).toBe(1)
    expect(pageOfIndex(49, 50)).toBe(1)
    expect(pageOfIndex(50, 50)).toBe(2)
    expect(pageOfIndex(349, 50)).toBe(7)
  })
})
