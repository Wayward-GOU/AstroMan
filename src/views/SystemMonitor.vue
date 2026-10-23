<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { message } from 'ant-design-vue'
import {
  PauseCircleOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  ThunderboltOutlined,
  HddOutlined,
  WifiOutlined,
  DashboardOutlined,
  CopyOutlined,
  GlobalOutlined,
} from '@ant-design/icons-vue'
import PageHeader from '@/components/PageHeader.vue'
import LineChart from '@/components/LineChart.vue'
import * as api from '@/api'
import type { MonitorLocalIp, MonitorProcess, MonitorSnapshot } from '@/types/models'

const HISTORY_LIMIT = 90

const snapshot = ref<MonitorSnapshot | null>(null)
const processes = ref<MonitorProcess[]>([])
const loading = ref(true)
const paused = ref(false)
const intervalSec = ref(3)
const processSort = ref<'cpu' | 'memory'>('cpu')

const cpuHistory = ref<number[]>([])
const memHistory = ref<number[]>([])
const gpuHistory = ref<number[]>([])
const downHistory = ref<number[]>([])
const upHistory = ref<number[]>([])
const readHistory = ref<number[]>([])
const writeHistory = ref<number[]>([])

let timer: number | null = null
let errorNotified = false

const COLORS = {
  cpu: '#3d6b8f',
  mem: '#7a8fa6',
  gpu: '#b07d33',
  down: '#3d6b8f',
  up: '#4f8a76',
  read: '#b07d33',
  write: '#b0524d',
}

function pushHistory(list: number[], value: number) {
  const next = [...list, Number.isFinite(value) ? value : 0]
  if (next.length > HISTORY_LIMIT) next.splice(0, next.length - HISTORY_LIMIT)
  return next
}

function gpuTotal(snap: MonitorSnapshot) {
  if (!snap.gpus.length) return 0
  return Math.max(...snap.gpus.map((g) => g.usage))
}

function netTotal(snap: MonitorSnapshot) {
  return snap.networks.reduce(
    (acc, item) => ({
      down: acc.down + item.receivedPerSec,
      up: acc.up + item.transmittedPerSec,
    }),
    { down: 0, up: 0 },
  )
}

function diskTotal(snap: MonitorSnapshot) {
  return snap.disks.reduce(
    (acc, item) => ({
      read: acc.read + item.readPerSec,
      write: acc.write + item.writePerSec,
    }),
    { read: 0, write: 0 },
  )
}

async function loadProcesses() {
  try {
    processes.value = await api.winstatsProcesses(12, processSort.value === 'memory')
  } catch {
    /* 进程扫描失败不影响主面板 */
  }
}

async function refresh() {
  try {
    const snap = await api.winstatsSnapshot()
    snapshot.value = snap
    cpuHistory.value = pushHistory(cpuHistory.value, snap.cpu.total)
    memHistory.value = pushHistory(memHistory.value, snap.memory.percent)
    gpuHistory.value = pushHistory(gpuHistory.value, gpuTotal(snap))

    const net = netTotal(snap)
    downHistory.value = pushHistory(downHistory.value, net.down / 1024 / 1024)
    upHistory.value = pushHistory(upHistory.value, net.up / 1024 / 1024)

    const disk = diskTotal(snap)
    readHistory.value = pushHistory(readHistory.value, disk.read / 1024 / 1024)
    writeHistory.value = pushHistory(writeHistory.value, disk.write / 1024 / 1024)

    errorNotified = false
  } catch (e) {
    if (!errorNotified) {
      errorNotified = true
      message.error(`读取系统状态失败：${String(e)}`)
    }
  } finally {
    loading.value = false
  }
}

async function tick() {
  if (paused.value) return
  await refresh()
  await loadProcesses()
}

function startTimer() {
  stopTimer()
  timer = window.setInterval(tick, intervalSec.value * 1000)
}

function stopTimer() {
  if (timer !== null) {
    window.clearInterval(timer)
    timer = null
  }
}

function togglePause() {
  paused.value = !paused.value
  if (paused.value) stopTimer()
  else {
    tick()
    startTimer()
  }
}

async function manualRefresh() {
  await tick()
}

function changeInterval() {
  // 采样间隔变化后历史点的横轴含义改变，重置曲线避免误导
  cpuHistory.value = []
  memHistory.value = []
  gpuHistory.value = []
  downHistory.value = []
  upHistory.value = []
  readHistory.value = []
  writeHistory.value = []
  if (!paused.value) startTimer()
}

async function changeSort(value: string | number) {
  processSort.value = value === 'memory' ? 'memory' : 'cpu'
  await loadProcesses()
}

onMounted(async () => {
  await tick()
  startTimer()
})

onUnmounted(stopTimer)

const labels = computed(() =>
  Array.from({ length: cpuHistory.value.length }, (_, i) => {
    const back = cpuHistory.value.length - 1 - i
    return back === 0 ? '现在' : `-${back * intervalSec.value}s`
  }),
)

const percentSeries = computed(() => {
  const series = [
    { name: 'CPU', color: COLORS.cpu, values: cpuHistory.value },
    { name: '内存', color: COLORS.mem, values: memHistory.value },
  ]
  if (gpuHistory.value.some((v) => v > 0)) {
    series.push({ name: 'GPU', color: COLORS.gpu, values: gpuHistory.value })
  }
  return series
})

const netSeries = computed(() => [
  { name: '下行', color: COLORS.down, values: downHistory.value },
  { name: '上行', color: COLORS.up, values: upHistory.value },
])

const diskSeries = computed(() => [
  { name: '读取', color: COLORS.read, values: readHistory.value },
  { name: '写入', color: COLORS.write, values: writeHistory.value },
])

const netNow = computed(() =>
  snapshot.value ? netTotal(snapshot.value) : { down: 0, up: 0 },
)

const diskNow = computed(() =>
  snapshot.value ? diskTotal(snapshot.value) : { read: 0, write: 0 },
)

const gpuNow = computed(() => (snapshot.value ? gpuTotal(snapshot.value) : 0))

const battery = computed(() => snapshot.value?.battery ?? null)

const localIps = computed<MonitorLocalIp[]>(() => snapshot.value?.ips ?? [])

const primaryIp = computed<MonitorLocalIp | null>(() => {
  const list = localIps.value
  return list.find((item) => item.primary) ?? list.find((item) => item.family === 'IPv4') ?? list[0] ?? null
})

const ipv4List = computed(() => localIps.value.filter((item) => item.family === 'IPv4'))

const ipv6List = computed(() => localIps.value.filter((item) => item.family === 'IPv6'))

const gateways = computed(() => {
  const set = new Set<string>()
  localIps.value.forEach((item) => {
    if (item.gateway) set.add(item.gateway)
  })
  return Array.from(set)
})

const onlineAdapters = computed(
  () => (snapshot.value?.networks ?? []).filter((net) => net.ipv4.length > 0).length,
)

const totalMemory = computed(() => snapshot.value?.memory.total ?? 0)

const totalDisk = computed(() =>
  (snapshot.value?.disks ?? []).reduce((acc, disk) => acc + disk.total, 0),
)

const totalDiskFree = computed(() =>
  (snapshot.value?.disks ?? []).reduce((acc, disk) => acc + disk.available, 0),
)

async function copyText(text?: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  } catch {
    message.warning('当前环境不允许访问剪贴板')
  }
}

function netIpLines(net: MonitorSnapshot['networks'][number]) {
  const lines: string[] = []
  if (net.ipv4.length) lines.push(`IPv4 ${net.ipv4.join(', ')}`)
  if (net.ipv6.length) lines.push(`IPv6 ${net.ipv6.join(', ')}`)
  if (net.gateway.length) lines.push(`网关 ${net.gateway.join(', ')}`)
  return lines
}

const overview = computed(() => {
  const snap = snapshot.value
  if (!snap) return []
  const cards = [
    {
      key: 'cpu',
      label: 'CPU',
      value: `${snap.cpu.total.toFixed(0)}%`,
      hint: `${snap.system.logicalCores} 线程 · ${(snap.cpu.frequencyMhz / 1000).toFixed(2)} GHz`,
      percent: snap.cpu.total,
    },
    {
      key: 'mem',
      label: '内存',
      value: `${snap.memory.percent.toFixed(0)}%`,
      hint: `${fmtBytes(snap.memory.used)} / ${fmtBytes(snap.memory.total)}`,
      percent: snap.memory.percent,
    },
  ]
  if (snap.gpus.length) {
    cards.push({
      key: 'gpu',
      label: 'GPU',
      value: `${gpuNow.value.toFixed(0)}%`,
      hint: snap.gpus.map((g) => g.name).join(' · '),
      percent: gpuNow.value,
    })
  }
  cards.push({
    key: 'down',
    label: '网络下行',
    value: fmtRate(netNow.value.down),
    hint: `上行 ${fmtRate(netNow.value.up)}`,
    percent: -1,
  })
  cards.push({
    key: 'disk',
    label: '磁盘 I/O',
    value: fmtRate(diskNow.value.read + diskNow.value.write),
    hint: `读 ${fmtRate(diskNow.value.read)} · 写 ${fmtRate(diskNow.value.write)}`,
    percent: -1,
  })
  if (battery.value?.present) {
    cards.push({
      key: 'battery',
      label: '电池',
      value: `${battery.value.percent}%`,
      hint: battery.value.onAc
        ? '已接通电源'
        : battery.value.minutesLeft > 0
          ? `剩余 ${battery.value.minutesLeft} 分钟`
          : '使用电池中',
      percent: battery.value.percent,
    })
  }
  return cards
})

const processesLabel = computed(() =>
  processSort.value === 'cpu' ? '按 CPU 占用排序' : '按内存占用排序',
)

function barTone(percent: number) {
  if (percent >= 85) return 'bad'
  if (percent >= 60) return 'warn'
  return 'ok'
}

function fmtBytes(v: number | undefined | null) {
  if (!v || v <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = v
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value >= 100 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`
}

function fmtRate(v: number) {
  return `${fmtBytes(v)}/s`
}

function fmtUptime(seconds: number) {
  if (!seconds) return '-'
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  if (days > 0) return `${days} 天 ${hours} 小时`
  if (hours > 0) return `${hours} 小时 ${minutes} 分`
  return `${minutes} 分`
}
</script>

<template>
  <div class="page">
    <PageHeader
      eyebrow="System Monitor"
      title="系统监视"
      subtitle="参考 winstats 的本机资源面板：CPU 每核心占用、内存、GPU、磁盘、网络与本机 IP"
    >
      <a-select v-model:value="intervalSec" size="small" style="width: 96px" @change="changeInterval">
        <a-select-option :value="3">3 秒</a-select-option>
        <a-select-option :value="5">5 秒</a-select-option>
        <a-select-option :value="10">10 秒</a-select-option>
      </a-select>
      <a-button size="small" @click="togglePause">
        <template #icon>
          <component :is="paused ? PlayCircleOutlined : PauseCircleOutlined" />
        </template>
        {{ paused ? '继续' : '暂停' }}
      </a-button>
      <a-button size="small" @click="manualRefresh">
        <template #icon><ReloadOutlined /></template>
        刷新
      </a-button>
    </PageHeader>

    <a-spin :spinning="loading">
      <template v-if="snapshot">
        <div class="overview">
          <div v-for="card in overview" :key="card.key" class="fm-card overview-card">
            <div class="ov-head">
              <span class="ov-label">{{ card.label }}</span>
              <span class="ov-value mono">{{ card.value }}</span>
            </div>
            <div v-if="card.percent >= 0" class="ov-track">
              <div
                class="ov-bar"
                :class="barTone(card.percent)"
                :style="{ width: `${Math.min(100, Math.max(0, card.percent))}%` }"
              />
            </div>
            <div class="ov-hint">{{ card.hint }}</div>
          </div>
        </div>

        <div class="fm-card ip-card">
          <div class="ip-main">
            <div class="ip-head">
              <span class="ip-icon"><GlobalOutlined /></span>
              <div>
                <div class="ip-label">本机 IP 地址</div>
                <div class="ip-sub">
                  {{ primaryIp ? `${primaryIp.adapter} · ${primaryIp.family}` : '未检测到可用 IP' }}
                </div>
              </div>
            </div>
            <div class="ip-value-row">
              <span class="ip-value mono">{{ primaryIp?.address || '-' }}</span>
              <a-button
                v-if="primaryIp"
                type="text"
                size="small"
                class="ip-copy"
                @click="copyText(primaryIp?.address)"
              >
                <template #icon><CopyOutlined /></template>
              </a-button>
            </div>
            <div class="ip-meta">
              <span>主机名 <b class="mono">{{ snapshot.system.hostName || '-' }}</b></span>
              <span>网关 <b class="mono">{{ gateways[0] || '-' }}</b></span>
            </div>
          </div>

          <div class="ip-list">
            <div class="ip-group">
              <div class="ip-group-title">IPv4（{{ ipv4List.length }}）</div>
              <div v-if="ipv4List.length" class="ip-tags">
                <button
                  v-for="item in ipv4List"
                  :key="`v4-${item.adapter}-${item.address}`"
                  class="ip-tag"
                  :class="{ primary: item.primary }"
                  :title="`${item.adapter} · 点击复制`"
                  @click="copyText(item.address)"
                >
                  <span class="mono">{{ item.address }}</span>
                  <em>{{ item.adapter }}</em>
                </button>
              </div>
              <div v-else class="ip-empty">暂无 IPv4 地址</div>
            </div>
            <div class="ip-group">
              <div class="ip-group-title">IPv6（{{ ipv6List.length }}）</div>
              <div v-if="ipv6List.length" class="ip-tags">
                <button
                  v-for="item in ipv6List"
                  :key="`v6-${item.adapter}-${item.address}`"
                  class="ip-tag"
                  :title="`${item.adapter} · 点击复制`"
                  @click="copyText(item.address)"
                >
                  <span class="mono">{{ item.address }}</span>
                  <em>{{ item.adapter }}</em>
                </button>
              </div>
              <div v-else class="ip-empty">暂无 IPv6 地址</div>
            </div>
          </div>
        </div>

        <div class="panel-grid">
          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">
                  <ThunderboltOutlined />
                  CPU 每核心占用
                </div>
                <div class="panel-sub">{{ snapshot.cpu.brand || '未知处理器' }} · {{ snapshot.cpu.vendor }}</div>
              </div>
              <span class="panel-note">{{ snapshot.cpu.total.toFixed(1) }}%</span>
            </div>
            <div class="core-grid">
              <div v-for="(usage, index) in snapshot.cpu.cores" :key="index" class="core-item">
                <span class="core-label mono">C{{ index }}</span>
                <div class="core-track">
                  <div
                    class="core-bar"
                    :class="barTone(usage)"
                    :style="{ height: `${Math.max(4, Math.min(100, usage))}%` }"
                  />
                </div>
                <span class="core-value mono">{{ usage.toFixed(0) }}</span>
              </div>
            </div>
            <div class="panel-foot">
              <span>{{ snapshot.system.physicalCores }} 物理核 / {{ snapshot.system.logicalCores }} 逻辑核</span>
              <span>{{ (snapshot.cpu.frequencyMhz / 1000).toFixed(2) }} GHz</span>
            </div>
          </div>

          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">
                  <DashboardOutlined />
                  内存与交换区
                </div>
                <div class="panel-sub">物理内存与页面文件的实时占用</div>
              </div>
              <span class="panel-note">{{ snapshot.memory.percent.toFixed(1) }}%</span>
            </div>

            <div class="mem-block">
              <div class="mem-row">
                <span class="mem-label">物理内存</span>
                <span class="mem-value mono">
                  {{ fmtBytes(snapshot.memory.used) }} / {{ fmtBytes(snapshot.memory.total) }}
                </span>
              </div>
              <div class="ov-track">
                <div
                  class="ov-bar"
                  :class="barTone(snapshot.memory.percent)"
                  :style="{ width: `${Math.min(100, snapshot.memory.percent)}%` }"
                />
              </div>
              <div class="mem-hint">可用 {{ fmtBytes(snapshot.memory.available) }}</div>
            </div>

            <div v-if="snapshot.swap" class="mem-block">
              <div class="mem-row">
                <span class="mem-label">交换区</span>
                <span class="mem-value mono">
                  {{ fmtBytes(snapshot.swap.used) }} / {{ fmtBytes(snapshot.swap.total) }}
                </span>
              </div>
              <div class="ov-track">
                <div
                  class="ov-bar"
                  :class="barTone(snapshot.swap.percent)"
                  :style="{ width: `${Math.min(100, snapshot.swap.percent)}%` }"
                />
              </div>
            </div>

            <div v-if="snapshot.gpus.length" class="gpu-block">
              <div class="mem-label">GPU 利用率</div>
              <div v-for="gpu in snapshot.gpus" :key="gpu.name" class="gpu-row">
                <span class="gpu-name">{{ gpu.name }}</span>
                <div class="ov-track">
                  <div
                    class="ov-bar"
                    :class="barTone(gpu.usage)"
                    :style="{ width: `${Math.min(100, gpu.usage)}%` }"
                  />
                </div>
                <span class="gpu-value mono">{{ gpu.usage.toFixed(0) }}%</span>
              </div>
            </div>
          </div>
        </div>

        <div class="charts">
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">负载趋势</span>
              <span class="chart-sub">CPU / 内存 / GPU 占用率（%）</span>
            </div>
            <LineChart :series="percentSeries" :labels="labels" unit="%" :area="false" :height="210" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">网络吞吐</span>
              <span class="chart-sub">所有网卡合计，单位 MB/s</span>
            </div>
            <LineChart :series="netSeries" :labels="labels" unit=" MB/s" :area="false" :height="210" />
          </div>
          <div class="fm-card chart-card">
            <div class="chart-head">
              <span class="chart-title">磁盘 I/O</span>
              <span class="chart-sub">所有磁盘合计，单位 MB/s</span>
            </div>
            <LineChart :series="diskSeries" :labels="labels" unit=" MB/s" :area="false" :height="210" />
          </div>
        </div>

        <div class="panel-grid">
          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">
                  <HddOutlined />
                  磁盘
                </div>
                <div class="panel-sub">容量占用与实时读写速率</div>
              </div>
            </div>
            <div v-if="snapshot.disks.length" class="list">
              <div v-for="disk in snapshot.disks" :key="disk.mount" class="list-row disk-row">
                <div class="row-main">
                  <span class="row-name">{{ disk.name }} <i v-if="disk.removable">可移动</i></span>
                  <span class="row-sub mono">{{ disk.mount }} · {{ disk.fileSystem || '未知格式' }}</span>
                </div>
                <div class="disk-meters">
                  <div class="ov-track">
                    <div
                      class="ov-bar"
                      :class="barTone(disk.percent)"
                      :style="{ width: `${Math.min(100, disk.percent)}%` }"
                    />
                  </div>
                  <div class="disk-numbers mono">
                    <span>{{ fmtBytes(disk.total - disk.available) }} / {{ fmtBytes(disk.total) }}</span>
                    <span class="io-read">读 {{ fmtRate(disk.readPerSec) }}</span>
                    <span class="io-write">写 {{ fmtRate(disk.writePerSec) }}</span>
                  </div>
                </div>
              </div>
            </div>
            <div v-else class="empty-text">未检测到磁盘</div>
          </div>

          <div class="fm-card panel">
            <div class="panel-head">
              <div>
                <div class="panel-title">
                  <WifiOutlined />
                  网络接口
                </div>
                <div class="panel-sub">按当前吞吐排序</div>
              </div>
            </div>
            <div v-if="snapshot.networks.length" class="list">
              <div v-for="net in snapshot.networks" :key="net.name" class="list-row net-row">
                <div class="row-main">
                  <span class="row-name">{{ net.name }}</span>
                  <span class="row-sub mono">{{ net.mac || '无 MAC 地址' }}</span>
                  <span v-for="line in netIpLines(net)" :key="line" class="row-ip mono">{{ line }}</span>
                </div>
                <div class="net-numbers">
                  <span class="io-read mono">↓ {{ fmtRate(net.receivedPerSec) }}</span>
                  <span class="io-write mono">↑ {{ fmtRate(net.transmittedPerSec) }}</span>
                  <span class="net-total mono">
                    累计 ↓{{ fmtBytes(net.totalReceived) }} · ↑{{ fmtBytes(net.totalTransmitted) }}
                  </span>
                </div>
              </div>
            </div>
            <div v-else class="empty-text">未检测到网络接口</div>
          </div>
        </div>

        <div class="fm-card panel process-panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">进程占用 Top 12</div>
              <div class="panel-sub">{{ processesLabel }}</div>
            </div>
            <a-segmented
              :value="processSort"
              :options="[
                { label: 'CPU', value: 'cpu' },
                { label: '内存', value: 'memory' },
              ]"
              size="small"
              @change="changeSort"
            />
          </div>
          <div class="data-table">
            <table>
              <thead>
                <tr>
                  <th class="col-name">进程</th>
                  <th class="col-pid">PID</th>
                  <th>CPU</th>
                  <th>内存</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="item in processes" :key="item.pid">
                  <td class="col-name cell-name" :title="item.name">{{ item.name }}</td>
                  <td class="col-pid mono">{{ item.pid }}</td>
                  <td class="mono">{{ item.cpu.toFixed(1) }}%</td>
                  <td class="mono">{{ fmtBytes(item.memory) }}</td>
                </tr>
              </tbody>
            </table>
            <div v-if="!processes.length" class="empty-text">暂无进程数据</div>
          </div>
        </div>

        <div class="fm-card panel">
          <div class="panel-head">
            <div>
              <div class="panel-title">系统信息</div>
              <div class="panel-sub">本机基础信息与运行状态</div>
            </div>
            <span class="panel-note">已运行 {{ fmtUptime(snapshot.system.uptimeSec) }}</span>
          </div>
          <div class="info-grid">
            <div class="info-item">
              <span class="info-label">操作系统</span>
              <span class="info-value">{{ snapshot.system.name }} {{ snapshot.system.osVersion }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">内核版本</span>
              <span class="info-value mono">{{ snapshot.system.kernelVersion || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">主机名</span>
              <span class="info-value mono">{{ snapshot.system.hostName || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">本机 IP</span>
              <span class="info-value mono">{{ primaryIp?.address || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">默认网关</span>
              <span class="info-value mono">{{ gateways[0] || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">已联网网卡</span>
              <span class="info-value mono">{{ onlineAdapters }} / {{ snapshot.networks.length }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">处理器</span>
              <span class="info-value" :title="snapshot.cpu.brand">{{ snapshot.cpu.brand || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">核心 / 线程</span>
              <span class="info-value mono">
                {{ snapshot.system.physicalCores }} / {{ snapshot.system.logicalCores }}
              </span>
            </div>
            <div class="info-item">
              <span class="info-label">当前频率</span>
              <span class="info-value mono">{{ (snapshot.cpu.frequencyMhz / 1000).toFixed(2) }} GHz</span>
            </div>
            <div class="info-item">
              <span class="info-label">物理内存</span>
              <span class="info-value mono">{{ fmtBytes(totalMemory) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">磁盘总容量</span>
              <span class="info-value mono">{{ fmtBytes(totalDisk) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">磁盘可用</span>
              <span class="info-value mono">{{ fmtBytes(totalDiskFree) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">系统已有运行时长</span>
              <span class="info-value mono">{{ fmtUptime(snapshot.system.uptimeSec) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">内存占用</span>
              <span class="info-value mono">
                {{ fmtBytes(snapshot.memory.used) }} / {{ fmtBytes(snapshot.memory.total) }}
              </span>
            </div>
            <div class="info-item">
              <span class="info-label">刷新间隔</span>
              <span class="info-value mono">{{ intervalSec }} 秒</span>
            </div>
          </div>
        </div>

        <div v-if="paused" class="paused-tip">已暂停自动刷新，点击「继续」恢复</div>
      </template>
    </a-spin>
  </div>
</template>

<style scoped>
/* ---------- 本机 IP ---------- */
.ip-card {
  margin-top: 16px;
  padding: 16px 18px;
  display: grid;
  grid-template-columns: minmax(240px, 300px) 1fr;
  gap: 18px;
  align-items: start;
}

.ip-main {
  padding-right: 18px;
  border-right: 1px dashed var(--fm-border);
}

.ip-head {
  display: flex;
  align-items: center;
  gap: 10px;
}

.ip-icon {
  width: 30px;
  height: 30px;
  border-radius: 9px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 14px;
  flex-shrink: 0;
}

.ip-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text);
}

.ip-sub {
  margin-top: 2px;
  font-size: 10px;
  color: var(--fm-muted);
}

.ip-value-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 9px 0 7px;
}

.ip-value {
  font-size: 21px;
  font-weight: 700;
  letter-spacing: 0.3px;
  color: var(--fm-text);
  word-break: break-all;
}

.ip-copy {
  color: var(--fm-muted);
}

.ip-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 5px 16px;
  font-size: 10px;
  color: var(--fm-muted);
}

.ip-meta b {
  font-weight: 600;
  color: var(--fm-text-2);
}

.ip-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}

.ip-group-title {
  margin-bottom: 6px;
  font-size: 10px;
  color: var(--fm-muted);
}

.ip-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.ip-tag {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  max-width: 100%;
  padding: 5px 9px;
  border: 1px solid var(--fm-border);
  border-radius: 8px;
  background: var(--fm-surface-alt);
  font-size: 11px;
  color: var(--fm-text);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.16s;
}

.ip-tag:hover {
  border-color: var(--fm-border-strong);
}

.ip-tag.primary {
  border-color: var(--fm-primary);
  background: var(--fm-primary-soft);
}

.ip-tag em {
  font-style: normal;
  font-size: 9px;
  color: var(--fm-muted);
}

.ip-empty {
  font-size: 10px;
  color: var(--fm-muted);
}

.row-ip {
  font-size: 9px;
  color: var(--fm-muted);
  word-break: break-all;
}

.overview {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 12px;
}

.overview-card {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.ov-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

.ov-label {
  font-size: 11px;
  color: var(--fm-muted);
}

.ov-value {
  font-size: 17px;
  font-weight: 700;
  color: var(--fm-text);
}

.ov-track {
  height: 6px;
  border-radius: 999px;
  background: var(--fm-surface-alt);
  overflow: hidden;
}

.ov-bar {
  height: 100%;
  border-radius: 999px;
  background: var(--fm-success);
  transition: width 0.35s ease;
}

.ov-bar.warn {
  background: var(--fm-warning);
}

.ov-bar.bad {
  background: var(--fm-danger);
}

.ov-hint {
  font-size: 10px;
  color: var(--fm-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.panel-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 16px;
}

.panel {
  padding: 16px 18px;
  min-width: 0;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.panel-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 13px;
  font-weight: 600;
}

.panel-title :deep(.anticon) {
  color: var(--fm-primary);
  font-size: 13px;
}

.panel-sub {
  margin-top: 3px;
  font-size: 10px;
  color: var(--fm-muted);
}

.panel-note {
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
}

.panel-foot {
  display: flex;
  justify-content: space-between;
  margin-top: 10px;
  padding-top: 9px;
  border-top: 1px dashed var(--fm-border);
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 每核心 ---------- */
.core-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(44px, 1fr));
  gap: 6px;
}

.core-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
}

.core-label {
  font-size: 9px;
  color: var(--fm-muted);
}

.core-track {
  width: 100%;
  height: 54px;
  border-radius: 5px;
  background: var(--fm-surface-alt);
  display: flex;
  align-items: flex-end;
  overflow: hidden;
}

.core-bar {
  width: 100%;
  border-radius: 4px 4px 0 0;
  background: var(--fm-success);
  transition: height 0.35s ease;
}

.core-bar.warn {
  background: var(--fm-warning);
}

.core-bar.bad {
  background: var(--fm-danger);
}

.core-value {
  font-size: 9px;
  color: var(--fm-text-2);
}

/* ---------- 内存 / GPU ---------- */
.mem-block + .mem-block,
.gpu-block {
  margin-top: 14px;
}

.mem-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  margin-bottom: 6px;
}

.mem-label {
  font-size: 11px;
  color: var(--fm-text-2);
  font-weight: 500;
}

.mem-value {
  font-size: 11px;
  color: var(--fm-muted);
}

.mem-hint {
  margin-top: 5px;
  font-size: 10px;
  color: var(--fm-muted);
}

.gpu-row {
  display: grid;
  grid-template-columns: 52px 1fr 40px;
  align-items: center;
  gap: 9px;
  margin-top: 7px;
}

.gpu-name {
  font-size: 10px;
  color: var(--fm-muted);
}

.gpu-value {
  font-size: 10px;
  color: var(--fm-text-2);
  text-align: right;
}

/* ---------- 图表 ---------- */
.charts {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
  margin-top: 16px;
}

.chart-card {
  padding: 14px 16px 6px;
  min-width: 0;
}

.chart-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.chart-title {
  font-size: 13px;
  font-weight: 600;
}

.chart-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

/* ---------- 列表 ---------- */
.list {
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.list-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 9px 12px;
  border-radius: 9px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
}

.row-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.row-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--fm-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-name i {
  font-style: normal;
  margin-left: 5px;
  padding: 0 5px;
  border-radius: 4px;
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-size: 9px;
  font-weight: 500;
}

.row-sub {
  font-size: 10px;
  color: var(--fm-muted);
}

.disk-meters {
  flex: 1;
  max-width: 240px;
  min-width: 120px;
}

.disk-numbers {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 5px;
  font-size: 9px;
  color: var(--fm-muted);
}

.io-read {
  color: var(--fm-warning);
}

.io-write {
  color: var(--fm-danger);
}

.net-numbers {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  font-size: 10px;
}

.net-total {
  width: 100%;
  text-align: right;
  font-size: 9px;
  color: var(--fm-muted);
}

/* ---------- 进程表 ---------- */
.process-panel {
  margin-top: 16px;
}

.data-table {
  overflow: auto;
}

.data-table table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}

.data-table th {
  padding: 7px 10px;
  text-align: right;
  font-weight: 500;
  color: var(--fm-muted);
  background: var(--fm-surface-alt);
  white-space: nowrap;
}

.data-table th:first-child {
  text-align: left;
}

.data-table td {
  padding: 6px 10px;
  text-align: right;
  border-bottom: 1px solid var(--fm-border);
  color: var(--fm-text-2);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.data-table tbody tr:hover td {
  background: var(--fm-surface-alt);
}

.col-name {
  text-align: left;
  width: 46%;
}

.cell-name {
  max-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--fm-text);
}

.col-pid {
  width: 76px;
}

/* ---------- 系统信息 ---------- */
.info-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 11px;
  border-radius: 9px;
  background: var(--fm-surface-alt);
  border: 1px solid var(--fm-border);
  min-width: 0;
}

.info-label {
  font-size: 10px;
  color: var(--fm-muted);
}

.info-value {
  font-size: 11px;
  font-weight: 600;
  color: var(--fm-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty-text {
  padding: 22px 0;
  text-align: center;
  font-size: 11px;
  color: var(--fm-muted);
}

.paused-tip {
  margin-top: 14px;
  padding: 9px 12px;
  border-radius: 8px;
  background: var(--fm-warning-soft);
  color: var(--fm-warning);
  font-size: 11px;
  text-align: center;
}

@media (max-width: 1280px) {
  .charts {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 1080px) {
  .panel-grid {
    grid-template-columns: 1fr;
  }
  .info-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
