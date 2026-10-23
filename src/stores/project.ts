import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { CollectionNode, Environment, Project } from '@/types/models'
import * as api from '@/api'

export const useProjectStore = defineStore('project', () => {
  const projects = ref<Project[]>([])
  const currentProject = ref<Project | null>(null)
  const tree = ref<CollectionNode[]>([])
  const environments = ref<Environment[]>([])
  const loading = ref(false)

  async function loadProjects() {
    loading.value = true
    try {
      projects.value = await api.projectList()
    } finally {
      loading.value = false
    }
  }

  async function openProject(id: number) {
    currentProject.value = projects.value.find((p) => p.id === id) ?? null
    if (!currentProject.value) {
      projects.value = await api.projectList()
      currentProject.value = projects.value.find((p) => p.id === id) ?? null
    }
    tree.value = await api.collectionTree(id)
    environments.value = await api.envList(id)
  }

  async function refreshTree(projectId: number) {
    tree.value = await api.collectionTree(projectId)
  }

  async function refreshEnvironments(projectId: number) {
    environments.value = await api.envList(projectId)
  }

  const activeEnvironment = ref<Environment | null>(null)
  function syncActiveEnvironment() {
    activeEnvironment.value = environments.value.find((e) => e.isActive) ?? environments.value[0] ?? null
  }

  return {
    projects,
    currentProject,
    tree,
    environments,
    activeEnvironment,
    loading,
    loadProjects,
    openProject,
    refreshTree,
    refreshEnvironments,
    syncActiveEnvironment,
  }
})
