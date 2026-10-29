import { describe, expect, it } from 'vitest'
import {
  isWriteTool,
  maskSensitive,
  TOOL_CATALOG,
  toolCatalogStats,
  toolNames,
  toolTimeoutOf,
  validateToolArgs,
} from '@/utils/aiTools'

describe('validateToolArgs', () => {
  it('缺失必填参数给出中文错误', () => {
    expect(validateToolArgs('project_create', {})).toBe('缺少必填参数 name')
  })

  it('枚举越界被拦截', () => {
    expect(
      validateToolArgs('send_request', { method: 'FETCH', url: 'https://a.com' }),
    ).toContain('必须是')
  })

  it('路径必须是本机绝对路径', () => {
    expect(validateToolArgs('project_export', { projectId: 1, path: 'out.json' })).toContain(
      '绝对路径',
    )
    expect(validateToolArgs('project_export', { projectId: 1, path: 'D:\\out.json' })).toBeNull()
  })

  it('数组长度与元素类型受约束', () => {
    expect(validateToolArgs('run_batch', { requestIds: [] })).toContain('至少需要 1 项')
    expect(validateToolArgs('run_batch', { requestIds: ['a'] })).toContain('数字数组')
    expect(validateToolArgs('run_batch', { requestIds: [1, 2] })).toBeNull()
  })

  it('数字上下限生效', () => {
    expect(
      validateToolArgs('send_request', {
        method: 'GET',
        url: 'https://a.com',
        timeoutMs: 60000,
      }),
    ).toContain('不能大于 30000')
  })

  it('布尔类型严格校验', () => {
    expect(
      validateToolArgs('send_request', { method: 'GET', url: 'https://a.com', cookieEnabled: 'yes' }),
    ).toContain('布尔值')
  })

  it('参数必须是对象', () => {
    expect(validateToolArgs('project_list', [] as unknown as Record<string, unknown>)).toBe(
      '参数必须是 JSON 对象',
    )
  })
})

describe('toolTimeoutOf', () => {
  it('普通工具 60 秒，导入导出 120 秒', () => {
    expect(toolTimeoutOf('project_list', {})).toBe(60_000)
    expect(toolTimeoutOf('history_export', { requestId: 1, path: 'D:\\a.csv' })).toBe(120_000)
  })

  it('send_request / run_batch 沿用入参上限', () => {
    expect(toolTimeoutOf('send_request', { timeoutMs: 10_000 })).toBe(15_000)
    expect(toolTimeoutOf('send_request', {})).toBe(35_000)
    // 入参本身超过 120 秒时也按 120 秒封顶
    expect(toolTimeoutOf('send_request', { timeoutMs: 999_000 })).toBe(125_000)
    expect(toolTimeoutOf('run_batch', { requestIds: [1, 2, 3], rounds: 2, timeoutMs: 10_000 })).toBe(30_000)
  })

  it('压测启停不包超时', () => {
    expect(toolTimeoutOf('stress_start', {})).toBeNull()
    expect(toolTimeoutOf('stress_stop', {})).toBeNull()
  })
})

describe('maskSensitive', () => {
  it('打码 Bearer token', () => {
    const out = maskSensitive('Authorization: Bearer sk-abcdefghijklmn')
    expect(out).not.toContain('abcdefghijklmn')
    expect(out).toContain('******')
  })

  it('打码 JSON 字段中的密钥', () => {
    const out = maskSensitive('{"api_key":"sk-1234567890abcd","name":"demo"}')
    expect(out).not.toContain('1234567890abcd')
    expect(out).toContain('"name":"demo"')
  })

  it('打码 URL 查询参数里的 token', () => {
    const out = maskSensitive('https://a.com/cb?token=abcdef123456&x=1')
    expect(out).not.toContain('abcdef123456')
    expect(out).toContain('x=1')
  })
})

describe('阶段四：工具生态', () => {
  const toolbox = [
    'toolbox_json_format',
    'toolbox_json_to_entity',
    'toolbox_json_to_sql',
    'toolbox_convert',
    'toolbox_timestamp',
    'toolbox_base64',
    'toolbox_hash',
    'toolbox_url',
    'toolbox_regex',
    'toolbox_jwt',
    'toolbox_diff',
    'toolbox_uuid',
  ]

  it('12 个工具箱工具与 7 个业务工具都已注册', () => {
    const names = toolNames()
    for (const name of toolbox) expect(names).toContain(name)
    for (const name of [
      'env_delete',
      'env_script_save',
      'report_delete',
      'report_requests',
      'history_stats',
      'run_list',
      'cookie_list',
    ]) {
      expect(names).toContain(name)
    }
    expect(names).toContain('tool_search')
  })

  it('写类工具需要确认，只读工具自动执行', () => {
    expect(isWriteTool('env_delete')).toBe(true)
    expect(isWriteTool('env_script_save')).toBe(true)
    expect(isWriteTool('report_delete')).toBe(true)
    expect(isWriteTool('toolbox_json_format')).toBe(false)
    expect(isWriteTool('history_stats')).toBe(false)
    expect(isWriteTool('cookie_list')).toBe(false)
    expect(isWriteTool('report_requests')).toBe(false)
  })

  it('长尾工具只列名字，目录靠 tool_search 收敛', () => {
    // 常用工具给完整说明
    expect(TOOL_CATALOG).toContain('project_list：')
    expect(TOOL_CATALOG).toContain('tool_search：')
    // 长尾工具只在名字清单里出现，不再占提示词字符
    expect(TOOL_CATALOG).toContain('toolbox_json_to_entity')
    expect(TOOL_CATALOG).not.toContain('把 JSON 转成 Java')
    // 目录整体保持精简
    expect(TOOL_CATALOG.length).toBeLessThan(3200)
    // 验收：注入字符数相比全量目录下降 ≥30%
    const stats = toolCatalogStats()
    expect(stats.ratio).toBeLessThanOrEqual(0.7)
  })

  it('新工具参数强校验', () => {
    expect(validateToolArgs('env_script_save', {})).toBe('缺少必填参数 id')
    expect(validateToolArgs('env_script_save', { id: 1, script: '' })).toBeNull()
    expect(validateToolArgs('toolbox_convert', { text: '{}', kind: 'toml' })).toContain('必须是')
    expect(
      validateToolArgs('report_requests', { id: 1, pageSize: 500 }),
    ).toContain('不能大于 200')
    expect(validateToolArgs('toolbox_hash', { text: 'a' })).toContain('缺少必填参数 algorithm')
    expect(validateToolArgs('tool_search', {})).toBe('缺少必填参数 keyword')
  })

  it('新工具默认 60 秒超时', () => {
    expect(toolTimeoutOf('toolbox_hash', { text: 'a', algorithm: 'SHA256' })).toBe(60_000)
    expect(toolTimeoutOf('report_requests', { id: 1 })).toBe(60_000)
  })
})
