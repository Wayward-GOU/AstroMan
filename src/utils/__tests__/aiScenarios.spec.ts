import { describe, expect, it } from 'vitest'
import { captureFlowPrompt } from '@/utils/aiScenarios'
import type { CaptureFlowDetail } from '@/types/capture'

function detail(overrides: Partial<CaptureFlowDetail> = {}): CaptureFlowDetail {
  return {
    id: 1,
    startedAt: 1_700_000_000_000,
    method: 'POST',
    scheme: 'https',
    host: 'api.example.com',
    path: '/login',
    httpVersion: 'HTTP/1.1',
    status: 200,
    durationMs: 128,
    requestSize: 64,
    responseSize: 2,
    contentType: 'application/json',
    clientAddr: '127.0.0.1:50000',
    error: null,
    mapped: false,
    modified: false,
    remote: null,
    requestHeaders: [
      { name: 'Authorization', value: 'Bearer abcdefghijklmn' },
      { name: 'X-Trace-Id', value: 'trace-123' },
    ],
    responseHeaders: [{ name: 'Set-Cookie', value: 'sid=topsecret' }],
    requestBody: {
      size: 42,
      truncated: false,
      binary: false,
      text: '{"username":"demo","password":"pass-123456"}',
      base64: null,
      contentType: 'application/json',
    },
    responseBody: {
      size: 2,
      truncated: false,
      binary: false,
      text: '{}',
      base64: null,
      contentType: 'application/json',
    },
    ...overrides,
  }
}

describe('captureFlowPrompt', () => {
  it('包含定位流量的关键字段', () => {
    const prompt = captureFlowPrompt(detail())
    expect(prompt).toContain('POST')
    expect(prompt).toContain('https://api.example.com/login')
    expect(prompt).toContain('状态码：200')
    expect(prompt).toContain('耗时：128 ms')
  })

  it('鉴权头与正文中的密码不会明文进入提示词', () => {
    const prompt = captureFlowPrompt(detail())
    expect(prompt).not.toContain('abcdefghijklmn')
    expect(prompt).not.toContain('pass-123456')
    expect(prompt).not.toContain('topsecret')
    expect(prompt).toContain('trace-123')
  })

  it('二进制正文只说明大小，不输出内容', () => {
    const prompt = captureFlowPrompt(
      detail({
        responseBody: {
          size: 2048,
          truncated: false,
          binary: true,
          text: null,
          base64: 'aGVsbG8=',
          contentType: 'application/octet-stream',
        },
      }),
    )
    expect(prompt).toContain('二进制内容 2048 字节')
    expect(prompt).not.toContain('aGVsbG8=')
  })
})
