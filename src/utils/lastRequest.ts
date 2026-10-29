/**
 * 记住每个项目最近打开的接口：
 * 从接口详情返回项目页时，自动选中该接口所在集合并高亮它，方便继续上次的工作。
 */
export interface LastRequestMemory {
  id: number
  collectionId: number
  name: string
  method: string
  at: number
}

const STORAGE_KEY = 'fm-last-request'

function readAll(): Record<string, LastRequestMemory> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<string, LastRequestMemory>
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

function writeAll(all: Record<string, LastRequestMemory>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all))
  } catch {
    // 存储空间不足时忽略，记忆失效不影响使用
  }
}

export function readLastRequest(projectId: number): LastRequestMemory | null {
  if (!projectId) return null
  return readAll()[String(projectId)] ?? null
}

export function rememberLastRequest(
  projectId: number,
  request: { id: number; collectionId?: number; name?: string; method?: string },
) {
  if (!projectId || !request.id) return
  const all = readAll()
  const previous = all[String(projectId)]
  all[String(projectId)] = {
    id: request.id,
    collectionId: request.collectionId || previous?.collectionId || 0,
    name: request.name || previous?.name || '未命名请求',
    method: request.method || previous?.method || '',
    at: Date.now(),
  }
  writeAll(all)
}

/** 删除接口或集合后清掉记忆；传入 id 时只清匹配的那一条 */
export function forgetLastRequest(projectId: number, id?: number) {
  if (!projectId) return
  const all = readAll()
  const current = all[String(projectId)]
  if (!current) return
  if (id !== undefined && current.id !== id) return
  delete all[String(projectId)]
  writeAll(all)
}
