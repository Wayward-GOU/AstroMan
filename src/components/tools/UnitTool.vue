<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { message } from 'ant-design-vue'
import { CopyOutlined } from '@ant-design/icons-vue'
import { convertAllUnits, convertUnit, UNIT_GROUPS, type UnitGroup } from '@/tools/calc'

const group = ref<UnitGroup>('length')
const value = ref<number | null>(1)
const from = ref('m')
const to = ref('cm')

const groups = Object.entries(UNIT_GROUPS).map(([key, item]) => ({
  value: key as UnitGroup,
  label: item.label,
}))

const units = computed(() => UNIT_GROUPS[group.value].units)

watch(group, () => {
  const list = units.value
  from.value = list[0].key
  to.value = list[1]?.key ?? list[0].key
})

const result = computed(() => {
  if (value.value === null || Number.isNaN(value.value)) return ''
  try {
    const converted = convertUnit(group.value, value.value, from.value, to.value)
    return Number(converted.toPrecision(12)).toString()
  } catch {
    return ''
  }
})

const allResults = computed(() => {
  if (value.value === null || Number.isNaN(value.value)) return []
  try {
    return UNIT_GROUPS[group.value].units.map((unit) => ({
      key: unit.key,
      label: unit.label,
      value: Number(convertUnit(group.value, value.value!, from.value, unit.key).toPrecision(12)).toString(),
    }))
  } catch {
    return []
  }
})

const allText = computed(() =>
  value.value === null ? '' : convertAllUnits(group.value, value.value, from.value),
)

async function copy(text: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  } catch {
    message.error('复制失败')
  }
}
</script>

<template>
  <div class="unit">
    <div class="unit-bar">
      <a-select v-model:value="group" size="small" style="width: 110px">
        <a-select-option v-for="item in groups" :key="item.value" :value="item.value">
          {{ item.label }}
        </a-select-option>
      </a-select>
      <a-input-number v-model:value="value" size="small" :style="{ width: '160px' }" placeholder="数值" />
      <a-select v-model:value="from" size="small" style="width: 170px">
        <a-select-option v-for="item in units" :key="item.key" :value="item.key">
          {{ item.label }}
        </a-select-option>
      </a-select>
      <span class="unit-arrow">→</span>
      <a-select v-model:value="to" size="small" style="width: 170px">
        <a-select-option v-for="item in units" :key="item.key" :value="item.key">
          {{ item.label }}
        </a-select-option>
      </a-select>
      <a-button size="small" :disabled="!allText" @click="copy(allText)">
        <CopyOutlined /> 复制全部
      </a-button>
    </div>

    <div class="unit-result">
      <span class="unit-result-label">换算结果</span>
      <span class="unit-result-value mono">{{ result || '—' }}</span>
      <span class="unit-result-unit">{{ units.find((item) => item.key === to)?.label }}</span>
    </div>

    <div class="unit-grid">
      <div v-for="item in allResults" :key="item.key" class="unit-item" :class="{ active: item.key === to }">
        <span class="unit-item-label">{{ item.label }}</span>
        <span class="unit-item-value mono">{{ item.value }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.unit {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.unit-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 10px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.unit-arrow {
  color: var(--fm-muted);
}

.unit-result {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 14px 16px;
  background: var(--fm-primary-soft);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.unit-result-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.unit-result-value {
  font-size: 20px;
  font-weight: 600;
  color: var(--fm-primary);
}

.unit-result-unit {
  font-size: 12px;
  color: var(--fm-text-2);
}

.unit-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 8px;
}

.unit-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 9px 12px;
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
}

.unit-item.active {
  border-color: var(--fm-primary);
}

.unit-item-label {
  font-size: 12px;
  color: var(--fm-muted);
}

.unit-item-value {
  font-size: 13px;
  color: var(--fm-text);
  word-break: break-all;
  text-align: right;
}
</style>
