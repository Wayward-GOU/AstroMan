import type { Component } from 'vue'

export type ToolCategoryId =
  | 'json'
  | 'format'
  | 'crypto'
  | 'text'
  | 'calc'
  | 'dev'
  | 'network'
  | 'qr'
  | 'reference'

export interface ToolCategory {
  id: ToolCategoryId
  name: string
  hint: string
}

export type ToolOptionType = 'select' | 'input' | 'switch' | 'number'

export interface ToolOption {
  key: string
  label: string
  type: ToolOptionType
  default: string | number | boolean
  options?: { label: string; value: string }[]
  placeholder?: string
  width?: number
  hint?: string
}

export interface SimpleTool {
  inputLabel?: string
  outputLabel?: string
  inputPlaceholder?: string
  outputPlaceholder?: string
  actionLabel?: string
  sample?: string
  options?: ToolOption[]
  rows?: number
  run: (input: string, options: Record<string, string | number | boolean>) => string | Promise<string>
}

export interface ToolDef {
  id: string
  name: string
  category: ToolCategoryId
  desc: string
  keywords?: string[]
  component?: () => Promise<{ default: Component }>
  props?: Record<string, unknown>
  simple?: SimpleTool
}

export type OptionValue = string | number | boolean
export type OptionState = Record<string, OptionValue>
