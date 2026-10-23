import { invoke } from '@tauri-apps/api/core'
import type { AppInfo, Project } from '@/types/models'

export const appInfo = () => invoke<AppInfo>('app_info')

export const projectList = () => invoke<Project[]>('project_list')

export const projectCreate = (name: string, description?: string) =>
  invoke<Project>('project_create', { name, description: description ?? null })

export const projectUpdate = (id: number, name: string, description?: string) =>
  invoke<void>('project_update', { id, name, description: description ?? null })

export const projectDelete = (id: number) => invoke<void>('project_delete', { id })
