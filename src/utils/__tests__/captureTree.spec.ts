import { describe, expect, it } from 'vitest'
import type { CaptureFlow } from '@/types/capture'
import {
  buildCaptureTree,
  captureFlowLabel,
  captureTreeKeys,
} from '@/utils/captureTree'

function flow(
  id: number,
  host: string,
  path: string,
  extra: Partial<CaptureFlow> = {},
): CaptureFlow {
  return {
    id,
    startedAt: id * 1000,
    method: 'GET',
    scheme: 'https',
    host,
    path,
    httpVersion: 'HTTP/1.1',
    status: 200,
    durationMs: 10,
    requestSize: 100,
    responseSize: 200,
    contentType: 'application/json',
    clientAddr: '127.0.0.1:5000',
    error: null,
    mapped: false,
    modified: false,
    remote: null,
    ...extra,
  }
}

describe('buildCaptureTree', () => {
  it('按域名分组，并把路径逐级拆成文件夹', () => {
    const tree = buildCaptureTree([
      flow(1, 'api.example.com', '/v1/users?page=1'),
      flow(2, 'api.example.com', '/v1/users'),
      flow(3, 'api.example.com', '/v1/orders'),
      flow(4, 'web.example.com', '/'),
    ])

    expect(tree.map((item) => item.host)).toEqual(['api.example.com', 'web.example.com'])
    const api = tree[0]
    expect(api.count).toBe(3)
    expect(api.children.map((item) => item.name)).toEqual(['v1'])

    const v1 = api.children[0]
    expect(v1.path).toBe('/v1')
    expect(v1.key).toBe('d:api.example.com:/v1')
    expect(v1.count).toBe(3)
    // 子目录按名称排序，请求按抓包顺序
    expect(v1.children.map((item) => item.name)).toEqual(['orders', 'users'])
    const users = v1.children[1]
    expect(users.key).toBe('d:api.example.com:/v1/users')
    expect(users.flows.map((item) => item.id)).toEqual([1, 2])
    expect(users.count).toBe(2)

    // 根路径请求直接挂在域名下
    expect(tree[1].flows.map((item) => item.id)).toEqual([4])
    expect(tree[1].children).toEqual([])
  })

  it('错误数沿目录向上汇总，包含 5xx', () => {
    const tree = buildCaptureTree([
      flow(1, 'a.com', '/x', { status: 500 }),
      flow(2, 'a.com', '/x/y', { error: 'boom', status: null }),
    ])
    expect(tree[0].errors).toBe(2)
    // /x 自身有一条 500，加上 /x/y 的错误向上汇总
    expect(tree[0].children[0].errors).toBe(2)
    expect(tree[0].children[0].children[0].errors).toBe(1)
  })

  it('同一路径的重复请求保留全部，忽略多余斜杠', () => {
    const tree = buildCaptureTree([
      flow(1, 'a.com', '//x//y/'),
      flow(2, 'a.com', '/x/y'),
    ])
    const x = tree[0].children[0]
    expect(x.name).toBe('x')
    expect(x.children[0].name).toBe('y')
    expect(x.children[0].flows.map((item) => item.id)).toEqual([1, 2])
  })

  it('空列表返回空树', () => {
    expect(buildCaptureTree([])).toEqual([])
  })
})

describe('captureFlowLabel', () => {
  it('叶子显示最后一段路径，保留查询串用于区分', () => {
    expect(captureFlowLabel(flow(1, 'a.com', '/v1/users?page=1'))).toBe('users?page=1')
    expect(captureFlowLabel(flow(1, 'a.com', '/v1/users/'))).toBe('users')
    expect(captureFlowLabel(flow(1, 'a.com', '/'))).toBe('/')
  })
})

describe('captureTreeKeys', () => {
  it('返回域名与所有目录的折叠键', () => {
    const keys = captureTreeKeys(
      buildCaptureTree([flow(1, 'a.com', '/x/y'), flow(2, 'b.com', '/')]),
    )
    expect(keys).toEqual(expect.arrayContaining(['h:a.com', 'd:a.com:/x', 'd:a.com:/x/y', 'h:b.com']))
    expect(keys).toHaveLength(4)
  })
})
