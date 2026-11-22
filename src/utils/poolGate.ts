import { h } from 'vue'
import { Modal } from 'ant-design-vue'
import * as api from '@/api'
import type { PoolBinding, PoolPreview } from '@/types/models'

/**
 * 预览行文案：自动注入列时展示列名列表，指定列时展示「池 → 变量」。
 */
function bindingLabel(item: PoolPreview['bindings'][number]): string {
  if (item.column === '*') {
    const targets = item.targets?.length ? item.targets.join('、') : '全部列'
    return `${item.pool} → 自动注入（${targets}）`
  }
  const column = item.column ? ` [${item.column}]` : ''
  return `${item.pool}${column} → ${item.target}`
}

/**
 * 开跑前的池强制预览（v1.1.2 第二批）
 *
 * - 没有绑定池时直接通过，保留无池快路径；
 * - 预览请求失败、绑定不完整或池没有可用条目时返回 false，调用方必须阻止启动；
 * - 其余情况弹出确认框展示池条目与 VU 覆盖，用户确认后才返回 true。
 */
export async function ensurePoolPreview(
  bindings: PoolBinding[] | null | undefined,
  vuCount: number,
  title = '执行前检查：参数 / Token 池',
): Promise<boolean> {
  const list = (bindings ?? []).filter(
    (binding) => Boolean(binding.pool?.trim()) || Boolean(binding.target?.trim()),
  )
  if (!list.length) return true

  const incomplete = list.find(
    (binding) =>
      !binding.pool?.trim() ||
      (binding.column !== '*' && !binding.target?.trim()),
  )
  if (incomplete) {
    Modal.error({
      title: '池绑定不完整，已阻止启动',
      content: '每个绑定都需要选择池并填写目标变量；选择「全部列」时目标变量可留空。',
    })
    return false
  }

  let preview: PoolPreview
  try {
    preview = await api.poolPreview(list, Math.max(0, Math.floor(vuCount)), false)
  } catch (error) {
    Modal.error({
      title: '池预览失败，已阻止启动',
      content: String(error),
    })
    return false
  }

  const emptyPools = preview.bindings.filter((item) => item.itemCount === 0).map((item) => item.pool)
  if (!preview.bindings.length || emptyPools.length) {
    Modal.error({
      title: '池预览未通过，已阻止启动',
      content: emptyPools.length
        ? `以下池没有可用条目：${Array.from(new Set(emptyPools)).join('、')}`
        : '没有可用的池绑定。',
    })
    return false
  }

  return new Promise<boolean>((resolve) => {
    Modal.confirm({
      title,
      width: 560,
      okText: '确认并继续',
      cancelText: '取消',
      content: h('div', { style: 'font-size:12.5px;line-height:1.7' }, [
        h(
          'div',
          { style: 'margin-bottom:6px;color:#6f7c85' },
          `本次 ${preview.vuCount} 个 VU，预览值均已脱敏：`,
        ),
        ...preview.bindings.map((item) =>
          h(
            'div',
            { style: 'display:flex;justify-content:space-between;gap:12px' },
            [
              h(
                'span',
                bindingLabel(item),
              ),
              h(
                'span',
                { style: 'color:#414d55;font-variant-numeric:tabular-nums' },
                `${item.itemCount} 条 / 覆盖 ${item.vusCovered} VU`,
              ),
            ],
          ),
        ),
        preview.problems.length
          ? h(
              'div',
              {
                style:
                  'margin-top:10px;padding:8px 10px;background:#fbf2e2;color:#9c6414;border-radius:4px',
              },
              [
                h('div', { style: 'font-weight:600;margin-bottom:4px' }, '预览提示'),
                ...preview.problems.map((problem) => h('div', problem)),
              ],
            )
          : null,
      ]),
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    })
  })
}
