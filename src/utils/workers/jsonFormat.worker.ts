import { parseLooseJson, type LooseRepair } from '../looseJson'

const ctx = self as unknown as {
  postMessage: (data: unknown) => void
  onmessage: ((event: MessageEvent) => void) | null
}

ctx.onmessage = (event: MessageEvent) => {
  const data = event.data as { id?: number; text?: string }
  if (typeof data?.id !== 'number') return
  const text = data.text ?? ''
  try {
    ctx.postMessage({ id: data.id, ok: true, result: JSON.stringify(JSON.parse(text), null, 2) })
  } catch (strictError) {
    // 严格解析失败后再宽松兜底：单引号 / 尾逗号 / 无引号 key / 注释由调用方决定是否提示
    let repairs: LooseRepair[]
    try {
      const loose = parseLooseJson(text)
      repairs = loose.repairs
      ctx.postMessage({
        id: data.id,
        ok: true,
        result: JSON.stringify(loose.value, null, 2),
        repairs,
      })
    } catch {
      ctx.postMessage({
        id: data.id,
        ok: false,
        error: strictError instanceof Error ? strictError.message : String(strictError),
      })
    }
  }
}

export {}
