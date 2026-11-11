import { invoke } from '@tauri-apps/api/core'
import type {
  BreakpointRule,
  CaptureCaInfo,
  CaptureFlow,
  CaptureFlowDetail,
  CaptureRepeatPayload,
  CaptureRepeatResult,
  CaptureStartOptions,
  CaptureStatus,
  InterceptView,
  MapRemoteRule,
  MapRule,
  RequestEdit,
  ResponseEdit,
  WeakNetwork,
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

export const captureExportHar = (
  path: string,
  ids?: number[],
  metadataOnly = false,
) =>
  invoke<void>('capture_export_har', {
    path,
    ...(ids?.length ? { ids } : {}),
    metadataOnly,
  })

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

/** 断点 / 本地映射 / 弱网的总览与挂起队列 */
export const captureInterceptState = () => invoke<InterceptView>('capture_intercept_state')

/** 保存断点开关与规则；只传一个字段表示部分更新 */
export const captureBreakpointSetRules = (options: {
  enabled?: boolean
  rules?: BreakpointRule[]
} = {}) =>
  invoke<InterceptView>('capture_breakpoint_set_rules', { ...options } as Record<string, unknown>)

/** 保存本地映射开关与规则 */
export const captureMapSetRules = (options: { enabled?: boolean; rules?: MapRule[] } = {}) =>
  invoke<InterceptView>('capture_map_set_rules', { ...options } as Record<string, unknown>)

/** 保存远程映射开关与规则 */
export const captureRemoteSetRules = (options: {
  enabled?: boolean
  rules?: MapRemoteRule[]
} = {}) =>
  invoke<InterceptView>('capture_remote_set_rules', { ...options } as Record<string, unknown>)

/** 保存弱网参数 */
export const captureWeakSet = (weak: WeakNetwork) =>
  invoke<InterceptView>('capture_weak_set', { weak })

/** 处理一条挂起的断点：continue / abort / edit */
export const captureBreakpointResolve = (payload: {
  pauseId: number
  action: 'continue' | 'abort' | 'edit'
  request?: RequestEdit
  response?: ResponseEdit
}) => invoke<InterceptView>('capture_breakpoint_resolve', { ...payload })

/** 放行全部挂起的断点（停止录制时也会自动释放） */
export const captureBreakpointReleaseAll = () =>
  invoke<InterceptView>('capture_breakpoint_release_all')
