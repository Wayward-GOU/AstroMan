/** 提问入库时的字符上限 */
export const STORED_PROMPT_LIMIT = 1000

/**
 * 压缩发给模型的提问：去掉零宽字符、行尾空白、连续空格和多余空行。
 * 代码块（``` 围栏内）保持原样，避免破坏脚本、JSON 与缩进。
 */
export function compactPrompt(input: string): string {
  const text = input.replace(/[\u200b-\u200d\ufeff]/g, '').replace(/\r\n?/g, '\n')
  return text
    .split(/(```[\s\S]*?```)/g)
    .map((part, index) => {
      if (index % 2 === 1) return part
      return part
        .replace(/[ \t]+$/gm, '')
        .replace(/(?<=\S)[ \t]{2,}/g, ' ')
        .replace(/\n{3,}/g, '\n\n')
    })
    .join('')
    .trim()
}

/** 提问入库时截断，超出部分直接丢弃，只为控制存储体积 */
export function clipStoredPrompt(input: string, limit = STORED_PROMPT_LIMIT): string {
  return input.length > limit ? input.slice(0, limit) : input
}
