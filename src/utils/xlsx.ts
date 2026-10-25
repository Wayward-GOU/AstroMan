/**
 * 极简 XLSX 生成器：不依赖第三方库，使用 STORE（不压缩）方式打包 ZIP。
 * 仅支持内联字符串与数字单元格，满足压测报告导出需求。
 */

export type CellValue = string | number | null | undefined

export interface XlsxSheet {
  name: string
  rows: CellValue[][]
}

const CRC_TABLE = (() => {
  const table = new Uint32Array(256)
  for (let i = 0; i < 256; i += 1) {
    let value = i
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1
    }
    table[i] = value >>> 0
  }
  return table
})()

function crc32(data: Uint8Array): number {
  let crc = 0xffffffff
  for (let i = 0; i < data.length; i += 1) {
    crc = CRC_TABLE[(crc ^ data[i]) & 0xff] ^ (crc >>> 8)
  }
  return (crc ^ 0xffffffff) >>> 0
}

function u16(value: number): number[] {
  return [value & 0xff, (value >>> 8) & 0xff]
}

function u32(value: number): number[] {
  return [value & 0xff, (value >>> 8) & 0xff, (value >>> 16) & 0xff, (value >>> 24) & 0xff]
}

function dosDateTime(date: Date): { time: number; date: number } {
  return {
    time: (date.getHours() << 11) | (date.getMinutes() << 5) | Math.floor(date.getSeconds() / 2),
    date: ((date.getFullYear() - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate(),
  }
}

function zipStore(files: { name: string; content: string }[]): Uint8Array {
  const encoder = new TextEncoder()
  const stamp = dosDateTime(new Date())
  const chunks: number[] = []
  const central: number[] = []
  let offset = 0

  for (const file of files) {
    const nameBytes = encoder.encode(file.name)
    const dataBytes = encoder.encode(file.content)
    const crc = crc32(dataBytes)

    const localHeader = [
      ...u32(0x04034b50),
      ...u16(20),
      ...u16(0x0800),
      ...u16(0),
      ...u16(stamp.time),
      ...u16(stamp.date),
      ...u32(crc),
      ...u32(dataBytes.length),
      ...u32(dataBytes.length),
      ...u16(nameBytes.length),
      ...u16(0),
      ...Array.from(nameBytes),
    ]
    chunks.push(...localHeader)
    for (let i = 0; i < dataBytes.length; i += 1) chunks.push(dataBytes[i])

    central.push(
      ...u32(0x02014b50),
      ...u16(20),
      ...u16(20),
      ...u16(0x0800),
      ...u16(0),
      ...u16(stamp.time),
      ...u16(stamp.date),
      ...u32(crc),
      ...u32(dataBytes.length),
      ...u32(dataBytes.length),
      ...u16(nameBytes.length),
      ...u16(0),
      ...u16(0),
      ...u16(0),
      ...u16(0),
      ...u32(0),
      ...u32(offset),
      ...Array.from(nameBytes),
    )
    offset += localHeader.length + dataBytes.length
  }

  const centralStart = offset
  chunks.push(...central)
  chunks.push(
    ...u32(0x06054b50),
    ...u16(0),
    ...u16(0),
    ...u16(files.length),
    ...u16(files.length),
    ...u32(central.length),
    ...u32(centralStart),
    ...u16(0),
  )
  return new Uint8Array(chunks)
}

function escapeXml(text: string): string {
  return text
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function columnName(index: number): string {
  let name = ''
  let value = index
  while (value >= 0) {
    name = String.fromCharCode(65 + (value % 26)) + name
    value = Math.floor(value / 26) - 1
  }
  return name
}

const MAX_CELL_TEXT = 32000

function cellXml(value: CellValue, ref: string): string {
  if (value === null || value === undefined || value === '') return ''
  if (typeof value === 'number' && Number.isFinite(value)) {
    return `<c r="${ref}"><v>${value}</v></c>`
  }
  const text = String(value)
  const clipped = text.length > MAX_CELL_TEXT ? `${text.slice(0, MAX_CELL_TEXT)}…` : text
  return `<c r="${ref}" t="inlineStr"><is><t xml:space="preserve">${escapeXml(clipped)}</t></is></c>`
}

function sheetXml(sheet: XlsxSheet): string {
  const lengths: number[] = []
  sheet.rows.forEach((row) => {
    row.forEach((cell, index) => {
      const text = cell === null || cell === undefined ? '' : String(cell)
      const width = Math.min(60, Math.max(8, [...text].length * 1.8))
      lengths[index] = Math.max(lengths[index] ?? 0, width)
    })
  })
  const cols = lengths.length
    ? `<cols>${lengths
        .map((width, index) => `<col min="${index + 1}" max="${index + 1}" width="${width.toFixed(1)}" customWidth="1"/>`)
        .join('')}</cols>`
    : ''

  const rows = sheet.rows
    .map((row, rowIndex) => {
      const cells = row
        .map((cell, colIndex) => cellXml(cell, `${columnName(colIndex)}${rowIndex + 1}`))
        .join('')
      return `<row r="${rowIndex + 1}">${cells}</row>`
    })
    .join('')

  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">${cols}<sheetData>${rows}</sheetData></worksheet>`
}

function cleanSheetName(name: string, index: number): string {
  const cleaned = name.replace(/[\\/?*[\]:]/g, ' ').trim().slice(0, 31)
  return cleaned || `Sheet${index + 1}`
}

/** 生成 .xlsx 文件二进制内容 */
export function buildXlsx(sheets: XlsxSheet[]): Uint8Array {
  const list = sheets.length ? sheets : [{ name: 'Sheet1', rows: [] }]
  const names = list.map((sheet, index) => cleanSheetName(sheet.name, index))

  const files: { name: string; content: string }[] = [
    {
      name: '[Content_Types].xml',
      content: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>${names
        .map(
          (_name, index) =>
            `<Override PartName="/xl/worksheets/sheet${index + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`,
        )
        .join('')}</Types>`,
    },
    {
      name: '_rels/.rels',
      content: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    },
    {
      name: 'xl/workbook.xml',
      content: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>${names
        .map(
          (name, index) =>
            `<sheet name="${escapeXml(name)}" sheetId="${index + 1}" r:id="rId${index + 1}"/>`,
        )
        .join('')}</sheets></workbook>`,
    },
    {
      name: 'xl/_rels/workbook.xml.rels',
      content: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${names
        .map(
          (_name, index) =>
            `<Relationship Id="rId${index + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${index + 1}.xml"/>`,
        )
        .join('')}</Relationships>`,
    },
  ]

  list.forEach((sheet, index) => {
    files.push({ name: `xl/worksheets/sheet${index + 1}.xml`, content: sheetXml(sheet) })
  })

  return zipStore(files)
}
