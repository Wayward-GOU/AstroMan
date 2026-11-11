import { describe, expect, it } from 'vitest'
import {
  classifyToolFailure,
  isWriteTool,
  maskSensitive,
  maskedArgsForLog,
  testCaseKindLabel,
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

  it('run_get 必须带 id，run_list / history_list 支持 offset 且受上限约束', () => {
    expect(validateToolArgs('run_get', {})).toBe('缺少必填参数 id')
    expect(validateToolArgs('run_get', { id: 12 })).toBeNull()
    expect(validateToolArgs('run_list', { requestId: 1, offset: 10 })).toBeNull()
    expect(validateToolArgs('run_list', { requestId: 1, offset: 500 })).toContain('不能大于 199')
    expect(validateToolArgs('history_list', { offset: 20 })).toBeNull()
  })
})

describe('工具目录', () => {
  it('run_get 进入核心目录，模型无需先检索就知道能读运行详情', () => {
    const names = toolNames()
    expect(names).toContain('run_get')
    expect(TOOL_CATALOG).toContain('run_get')
  })

  it('系统 / 应用 / 记事本增量工具已注册，snooze 属于写工具', () => {
    const names = toolNames()
    for (const tool of [
      'system_snapshot',
      'system_process_list',
      'app_info',
      'notes_counts',
      'notes_tags',
      'notes_snooze',
    ]) {
      expect(names).toContain(tool)
    }
    expect(isWriteTool('notes_snooze')).toBe(true)
    expect(isWriteTool('system_process_list')).toBe(false)
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

describe('classifyToolFailure', () => {
  it('写工具超时：结果未知且禁止自动重试', () => {
    expect(classifyToolFailure('project_import', true)).toEqual({
      outcome: 'unknown',
      retryable: false,
    })
    expect(classifyToolFailure('history_export', true)).toEqual({
      outcome: 'unknown',
      retryable: false,
    })
    expect(classifyToolFailure('send_request', true)).toEqual({ outcome: 'timeout', retryable: true })
  })

  it('只读工具超时仍可重试', () => {
    expect(classifyToolFailure('project_list', true)).toEqual({
      outcome: 'timeout',
      retryable: true,
    })
  })

  it('普通失败与成功保持 error / ok', () => {
    expect(classifyToolFailure('project_list', false)).toEqual({ outcome: 'error', retryable: true })
    expect(classifyToolFailure('project_import', false)).toEqual({ outcome: 'error', retryable: true })
    expect(classifyToolFailure('project_list', false, true)).toEqual({ outcome: 'ok', retryable: true })
  })
})

describe('maskedArgsForLog', () => {
  it('落库前打码 Header 与 Body 里的密钥', () => {
    const out = maskedArgsForLog('send_request', {
      method: 'POST',
      url: 'https://a.com/login',
      headers: { Authorization: 'Bearer abcdef123456', Cookie: 'sid=abcdef123456' },
      body: '{"username":"demo","password":"hunter2secret"}',
    })
    expect(out).not.toContain('abcdef123456')
    expect(out).not.toContain('hunter2secret')
    expect(out).toContain('https://a.com/login')
  })

  it('环境变量值按密钥处理', () => {
    const out = maskedArgsForLog('env_var_save', {
      projectId: 1,
      envId: 2,
      variables: [{ key: 'API_TOKEN', value: 'abcdef123456' }],
    })
    expect(out).not.toContain('abcdef123456')
    expect(out).toContain('API_TOKEN')
  })

  it('先脱敏再截断，避免密钥被切成半截漏出', () => {
    const out = maskedArgsForLog('send_request', {
      method: 'GET',
      url: `https://a.com/cb?token=abcdef123456&pad=${'a'.repeat(3000)}`,
    })
    expect(out.length).toBeLessThan(2100)
    expect(out).not.toContain('abcdef123456')
    expect(out).toContain('已截断')
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
    // 目录整体保持精简（v1.0.9 新增抓包 7 + 测试 10 + 批量创建 1 个长尾工具，
    // W5 列表工具补 offset / limit 参数；只增加名字与参数名，上限随清单放宽）
    expect(TOOL_CATALOG.length).toBeLessThan(4400)
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

  it('测试域工具：写入需确认、启动不限时、参数强校验', () => {
    expect(isWriteTool('test_suite_create')).toBe(true)
    expect(isWriteTool('test_case_update')).toBe(true)
    expect(isWriteTool('test_run_start')).toBe(true)
    expect(isWriteTool('test_run_stop')).toBe(true)
    expect(isWriteTool('test_suite_list')).toBe(false)
    expect(isWriteTool('test_run_get')).toBe(false)
    expect(toolTimeoutOf('test_run_start', { suiteId: 1 })).toBeNull()
    expect(validateToolArgs('test_suite_get', {})).toBe('缺少必填参数 id')
    expect(validateToolArgs('test_case_create', { suiteId: 1, name: 'x', kind: 'network' })).toContain(
      '必须是',
    )
    expect(validateToolArgs('test_run_get', { runId: 12 })).toBeNull()
  })

  it('用例类型有中文名，未知值返回空串由调用方兜底', () => {
    expect(testCaseKindLabel('request')).toBe('接口用例')
    expect(testCaseKindLabel('python')).toBe('脚本用例')
    expect(testCaseKindLabel('stress')).toBe('压测用例')
    expect(testCaseKindLabel('REQUEST')).toBe('接口用例')
    expect(testCaseKindLabel('network')).toBe('')
    expect(testCaseKindLabel(undefined)).toBe('')
  })
})

describe('P0：抓包域与测试域补齐', () => {
  it('抓包 7 个工具已注册，启停 / 重发 / 导出需确认', () => {
    const names = toolNames()
    for (const name of [
      'capture_status',
      'capture_flow_list',
      'capture_flow_get',
      'capture_start',
      'capture_stop',
      'capture_repeat',
      'capture_export',
    ]) {
      expect(names).toContain(name)
    }
    expect(isWriteTool('capture_start')).toBe(true)
    expect(isWriteTool('capture_stop')).toBe(true)
    expect(isWriteTool('capture_repeat')).toBe(true)
    expect(isWriteTool('capture_export')).toBe(true)
    expect(isWriteTool('capture_status')).toBe(false)
    expect(isWriteTool('capture_flow_list')).toBe(false)
    expect(isWriteTool('capture_flow_get')).toBe(false)
    expect(toolTimeoutOf('capture_export', { path: 'D:\\a.har' })).toBe(120_000)
  })

  it('测试域补齐 10 个工具：写入需确认、只读自动执行', () => {
    const names = toolNames()
    for (const name of [
      'test_dataset_list',
      'test_dataset_get',
      'test_dataset_save',
      'test_dataset_delete',
      'test_suite_import',
      'test_suite_export',
      'test_run_list',
      'test_artifact_list',
      'test_artifact_export',
      'test_preflight',
    ]) {
      expect(names).toContain(name)
    }
    expect(isWriteTool('test_dataset_save')).toBe(true)
    expect(isWriteTool('test_dataset_delete')).toBe(true)
    expect(isWriteTool('test_suite_import')).toBe(true)
    expect(isWriteTool('test_suite_export')).toBe(true)
    expect(isWriteTool('test_artifact_export')).toBe(true)
    expect(isWriteTool('test_dataset_list')).toBe(false)
    expect(isWriteTool('test_dataset_get')).toBe(false)
    expect(isWriteTool('test_run_list')).toBe(false)
    expect(isWriteTool('test_artifact_list')).toBe(false)
    expect(isWriteTool('test_preflight')).toBe(false)
    expect(toolTimeoutOf('test_suite_import', { path: 'D:\\a.json' })).toBe(120_000)
    expect(toolTimeoutOf('test_suite_export', { suiteId: 1, path: 'D:\\a.json' })).toBe(120_000)
    expect(toolTimeoutOf('test_artifact_export', { artifactId: 1, path: 'D:\\a.log' })).toBe(120_000)
    expect(toolTimeoutOf('test_preflight', { suiteId: 1 })).toBe(60_000)
  })

  it('request_batch_create 已注册且参数受约束', () => {
    expect(toolNames()).toContain('request_batch_create')
    expect(isWriteTool('request_batch_create')).toBe(true)
    expect(validateToolArgs('request_batch_create', { collectionId: 1, requests: [] })).toContain(
      '至少需要 1 项',
    )
    expect(validateToolArgs('request_batch_create', { collectionId: 1, requests: [{}] })).toBeNull()
    expect(toolTimeoutOf('request_batch_create', { collectionId: 1, requests: [{}] })).toBe(60_000)
  })

  it('send_request 可以只传 requestId，走已保存请求的脚本链路', () => {
    expect(validateToolArgs('send_request', { requestId: 12 })).toBeNull()
    expect(validateToolArgs('send_request', {})).toBeNull()
    expect(validateToolArgs('send_request', { method: 'FETCH', url: 'https://a.com' })).toContain(
      '必须是',
    )
  })

  it('新工具参数强校验', () => {
    expect(validateToolArgs('capture_flow_get', {})).toBe('缺少必填参数 id')
    expect(validateToolArgs('capture_export', { path: 'out.har' })).toContain('绝对路径')
    expect(validateToolArgs('capture_export', { path: 'D:\\out.har', format: 'json' })).toContain(
      '必须是',
    )
    expect(validateToolArgs('capture_flow_list', { status: 99 })).toContain('不能小于 100')
    expect(validateToolArgs('test_dataset_save', { format: 'yaml' })).toContain('必须是')
    expect(validateToolArgs('test_suite_export', {})).toBe('缺少必填参数 suiteId')
    expect(validateToolArgs('test_artifact_export', { artifactId: 1, path: 'a.log' })).toContain(
      '绝对路径',
    )
    expect(validateToolArgs('test_preflight', {})).toBe('缺少必填参数 suiteId')
    expect(validateToolArgs('test_run_list', { size: 500 })).toContain('不能大于 100')
  })
})

describe('单一定义源（AI-11）', () => {
  it('每个工具都有参数 Schema，且能派生原生 function calling 定义', async () => {
    const { nativeToolDefs, toolParameterSchema, toolArgsSignature } = await import('@/utils/aiTools')
    const defs = nativeToolDefs()
    expect(defs.length).toBe(toolNames().length)
    for (const def of defs) {
      const schema = toolParameterSchema(def.name)
      expect(schema.type).toBe('object')
      expect(schema.properties).toBeTypeOf('object')
      expect(Array.isArray(schema.required)).toBe(true)
      expect(def.description.length).toBeGreaterThan(0)
      expect(toolArgsSignature(def.name).length).toBeGreaterThan(0)
    }
  })

  it('Schema 派生枚举、必填与上下界', async () => {
    const { toolParameterSchema, toolArgsSignature } = await import('@/utils/aiTools')
    const create = toolParameterSchema('request_create')
    expect((create.required as string[]).sort()).toEqual(['collectionId', 'method', 'name', 'url'])
    const props = create.properties as Record<string, Record<string, unknown>>
    expect(props.method.enum).toContain('POST')
    expect(props.timeoutMs).toBeUndefined()

    const send = toolParameterSchema('send_request')
    const sendProps = send.properties as Record<string, Record<string, unknown>>
    expect(sendProps.timeoutMs.minimum).toBe(1000)
    expect(sendProps.timeoutMs.maximum).toBe(30000)

    expect(toolArgsSignature('project_create')).toBe('{ name, description? }')
    expect(toolArgsSignature('request_create')).toContain('method: enum')
    expect(toolArgsSignature('send_request')).toContain('timeoutMs?')
    expect(toolArgsSignature('project_list')).toBe('无')
  })
})
