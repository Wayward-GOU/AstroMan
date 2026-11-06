const ctx = self as unknown as {
  postMessage: (data: unknown) => void
  onmessage: ((event: MessageEvent) => void) | null
}

ctx.onmessage = (event: MessageEvent) => {
  const data = event.data as { id?: number; text?: string }
  if (typeof data?.id !== 'number') return
  try {
    const formatted = JSON.stringify(JSON.parse(data.text ?? ''), null, 2)
    ctx.postMessage({ id: data.id, ok: true, result: formatted })
  } catch (error) {
    ctx.postMessage({
      id: data.id,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    })
  }
}

export {}
