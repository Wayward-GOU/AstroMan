import { invoke } from '@tauri-apps/api/core'
import type {
  CaptureCaInfo,
  CaptureFlow,
  CaptureFlowDetail,
  CaptureRepeatPayload,
  CaptureRepeatResult,
  CaptureStartOptions,
  CaptureStatus,
} from '@/types/capture'

export const captureStart = (options: CaptureStartOptions = {}) =>
  invoke<CaptureStatus>('capture_start', { ...options } as Record<string, unknown>)

export const captureStop = () => invoke<CaptureStatus>('capture_stop')

export const captureStatus = () => invoke<CaptureStatus>('capture_status')

export const captureFlows = (limit = 1000) =>
  invoke<CaptureFlow[]>('capture_flows', { limit })

export const captureFlowDetail = (id: number) =>
  invoke<CaptureFlowDetail>('capture_flow_detail', { id })

export const captureFlowClear = () => invoke<CaptureStatus>('capture_flow_clear')

/** 手机抓包用：电脑当前可用的局域网 IPv4 */
export const captureLanIps = () => invoke<string[]>('capture_lan_ips')

/** 重发：可覆盖方法 / URL / 请求头 / 请求体；录制中会录制新流量 */
export const captureRepeat = (payload: CaptureRepeatPayload) =>
  invoke<CaptureRepeatResult>('capture_repeat', { ...payload } as Record<string, unknown>)

export const captureExportHar = (path: string, ids?: number[]) =>
  invoke<void>('capture_export_har', ids?.length ? { path, ids } : { path })

export const captureExportZip = (path: string, ids?: number[]) =>
  invoke<void>('capture_export_zip', ids?.length ? { path, ids } : { path })

export const captureSaveBody = (id: number, part: 'request' | 'response', path: string) =>
  invoke<void>('capture_save_body', { id, part, path })

export const captureCaInfo = () => invoke<CaptureCaInfo>('capture_ca_info')

export const captureCaGenerate = (force = false) =>
  invoke<CaptureCaInfo>('capture_ca_generate', { force })

export const captureCaInstall = () => invoke<CaptureCaInfo>('capture_ca_install')

export const captureCaExport = (path: string) =>
  invoke<void>('capture_ca_export', { path })

export const captureSysproxyEnable = () =>
  invoke<CaptureStatus>('capture_sysproxy_enable')

export const captureSysproxyDisable = () =>
  invoke<CaptureStatus>('capture_sysproxy_disable')
