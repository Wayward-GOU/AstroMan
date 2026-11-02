---
name: astroman-toolbox
description: AstroMan 离线工具箱（9 分类 / 92 个工具）的开发规范。新增或修改工具、调整注册表与搜索、改收藏与最近使用、同步工具清单文档时使用。触发词：工具箱、离线工具、registry、ToolDef、ToolWorkbench、JSONPath、JWT 验签、TOTP、造数、脱敏、二维码。
---

# 离线工具箱

92 个工具、9 个分类，全部本机计算，不联网（IP 查询是唯一例外，走用户配置的网络）。所有实现必须有纯函数层，页面只负责交互。

## 规模与分类（改数量先改这里，再改 `docs/toolbox.md`）

| 分类 id | 名称 | 数量 |
|---|---|---|
| `json` | JSON | 21 |
| `format` | 格式化转换 | 2 |
| `crypto` | 加密编码 | 22 |
| `text` | 文本处理 | 14 |
| `calc` | 计算换算 | 10 |
| `dev` | 开发辅助 | 7 |
| `network` | 网络工具 | 7 |
| `qr` | 二维码 | 2 |
| `reference` | 查询手册 | 7 |

合计 **92**。核对方法（`id:` 同时匹配分类与工具，别被 101 这个数骗到）：

```bash
grep -o "id: '[a-z0-9-]*'" src/tools/registry.ts | sort -u | wc -l      # 101 = 92 工具 + 9 分类
grep -o "category: '[a-z]*'" src/tools/registry.ts | sort | uniq -c     # 每个分类的工具数
```

## 代码地图

| 位置 | 职责 |
|---|---|
| `src/tools/types.ts` | 契约：`ToolDef` / `ToolCategory` / `SimpleTool` / `ToolOption` |
| `src/tools/registry.ts` | `TOOLS` 数组、`TOOL_CATEGORIES`、`TOOL_MAP`、`searchTools()`、`toolsByCategory()` |
| `src/tools/<域>.ts` | 纯函数实现：`transforms` / `converters` / `crypto` / `encoding` / `calc` / `entity` / `devtools` / `network` / `netparse` / `jsonpath` / `jsondiff` / `schema` / `jwtverify` / `otp` / `mask` / `datagen` / `idgen` / `qrcode` / `formatters` / `textbox` / `reference` |
| `src/components/tools/` | 20 个复杂工具组件 + `ToolWorkbench.vue`（纯函数工具的通用外壳） |
| `src/views/Tools.vue` | 搜索、分类导航、收藏 / 最近使用、`?tool=<id>` 深链 |
| `src/stores/toolPrefs.ts` | 收藏（上限 20）与最近使用（上限 10），键 `astroman.tool.favorites` / `astroman.tool.recent` |
| `src/tools/__tests__/toolbox.spec.ts` | 40 条单测：JSONPath、diff、Schema、编码往返、脱敏、造数校验位、ULID、UA / HTTP 解析、TOTP 向量、JWT 验签 |
| `docs/toolbox.md` | 面向用户的清单，按分类逐条列能力 |

## 两种接入方式

**纯函数工具**（当前 64 个）——`simple.run` 同步或异步返回字符串，由 `ToolWorkbench` 统一渲染：

```ts
{
  id: 'ua-parse',
  name: 'User-Agent 解析',
  category: 'network',
  desc: '识别浏览器、系统、设备、渲染引擎与 CPU 架构',
  keywords: ['user-agent', 'ua', '浏览器', '设备'],
  aliases: ['ua解析', 'useragent'],
  simple: {
    inputLabel: 'User-Agent',
    outputLabel: '解析结果',
    actionLabel: '解析',
    sample: 'Mozilla/5.0 ...',
    run: (input) => parseUserAgent(input),
  },
}
```

需要多个输入框时用 `simple.options`（`select` / `input` / `switch` / `number`，每项有 `key` / `label` / `default`，可给 `options` 枚举）；`run(input, options)` 第二参即当前选项值。

**组件工具**（当前 28 个）——需要文件、画布、多栏联动或自定义排版时用 `component: () => import('@/components/tools/XxxTool.vue')` 懒加载，`props` 传初始值。组件里也**不要**写业务算法，调 `src/tools/*.ts`。

## 新增一个工具

1. 在对应域文件写纯函数（签名明确、输入输出都是字符串 / 简单结构），复杂逻辑拆小函数便于测
2. 在 `registry.ts` 的 `TOOLS` 末尾按分类追加一条：`id`（kebab-case、全局唯一）、`name`、`category`、`desc`（一句话说清「做什么 + 输出什么」）、`keywords`、`aliases`
3. 选接入方式：能用一个输入框 + 一个按钮搞定就写 `simple`；否则在 `src/components/tools/` 建组件并 `component: () => import(...)`
4. 在 `toolbox.spec.ts` 补用例：正常路径、边界（空输入 / 超长 / 非法格式）、往返一致性（编码→解码）
5. 访问工具：分类导航 + 搜索 + `?tool=<id>` 深链
6. 同步 `docs/toolbox.md` 的分类条目与首行总数；分类数量变了还要改本文顶部表格
7. 验证：

```bash
npx vitest run src/tools/__tests__/toolbox.spec.ts --pool=forks --poolOptions.forks.singleFork
npm run build
```

## 契约与坑

1. **`id` 与分类 id 共用一个正则空间**：统计时别把 9 个分类算成工具；新增 id 不能和分类 id 重名
2. **纯函数文件不许 import Vue**：`types.ts` 里的 `import type { Component }` 是唯一的类型依赖；`src/tools/*.ts` 一旦引运行时 Vue，vitest（node 环境）就跑不了
3. **搜索命中范围固定**：`name + desc + keywords + aliases + 分类名`，都转小写匹配。用户反馈「搜不到」时优先补 `aliases`，不要把关键词塞进 `name`
4. **收藏 / 最近使用走后端无关的 localStorage**：`useToolPrefsStore()` 统一维护，别在组件里自己读写
5. **大输入要留意响应式开销**：超长文本先截断或提示，别把几十 MB 的字符串塞进 `ref` 再反复计算；图片 / 文件类工具走 Tauri dialog 与后端命令
6. **二维码依赖 `qrcode` + `jsqr`**：生成与识别都在本机，别换成需要联网的 API
7. **文档与代码同步**：`docs/toolbox.md` 的数量、分类与条目要和 `registry.ts` 一致；`CHANGELOG.md` 记新增工具时写清分类与总数变化（如「75 → 92」）
