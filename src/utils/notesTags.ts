/** 标签配色：同名标签在任何位置都得到同一个色号，不依赖创建顺序 */
export const TAG_TONES = 8

export function tagTone(name: string): number {
  let hash = 0
  for (let index = 0; index < name.length; index += 1) {
    hash = (hash * 31 + name.charCodeAt(index)) >>> 0
  }
  return hash % TAG_TONES
}

export function tagToneClass(name: string): string {
  return `tone-${tagTone(name)}`
}
