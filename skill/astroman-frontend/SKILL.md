---
name: astroman-frontend
description: AstroMan 前端开发规范（Vue 3 + Ant Design Vue + Pinia + 设计系统 + 脚本引擎/导出工具）。改页面、改样式主题、加组件、调脚本能力、改报告导出时使用。触发词：前端、Vue、页面、样式、主题、暗色、CSS 变量、脚本、加密、导出报告。
---

# 前端规范

入口 `src/main.ts`：`createPinia` → `router` → `app.use(Antd)`，样式顺序固定为 `ant-design-vue/dist/reset.css` 之后加载 `styles/main.css`。
启动前会执行 `applyPlatformAttributes()`（给 `<html>` 打平台标记）与 `initTheme()`（恢复主题）。

## 设计系统

- 所有颜色/圆角/阴影只用 `main.css` 里的 `--fm-*` 变量，**不要写死 hex**
- 主要 token：`--fm-primary` / `-dark` / `-soft`、`--fm-success|warning|danger|purple`（各有 `-soft`）、`--fm-bg`、`--fm-surface`、`--fm-surface-alt`、`--fm-border` / `-strong`、`--fm-text` / `-2` / `--fm-muted`、`--fm-radius-sm|`radius`|lg`、`--fm-shadow-sm|`shadow`|lg`、侧边栏一组 `--fm-sidebar-*`、代码区 `--fm-code-bg` / `--fm-gutter-bg`
- 代码/JSON 专用色（每套主题都定义了同一批）：`--fm-json-key`（字段名）、`--fm-json-string`（字符串）、`--fm-json-number`（数字）、`--fm-json-bool`（布尔与 `{{变量}}`）、`--fm-json-punct`（括号、逗号、冒号）；不要在组件里为暗色主题单独写死颜色
- 主题：`stores/theme.ts` 定义 `ThemeId`（**23 套**，16 浅 / 7 深），每套 `ThemeDefinition` 用同一批 35 个 `--fm-*` 颜色 / 阴影变量覆盖（HTTP 方法色与圆角沿用 `main.css`）；深色的 7 套是 `midnight` / `dusk` / `indigo` / `plum` / `pine` / `slate` / `aurora`。新增主题只加一个定义并追加到 `themes` 数组（顺序即设置页顺序），不要在组件里判断主题名
- 深色主题靠 `data-theme` + `applyTheme()` 统一换变量，需要针对性微调时用 `html[data-theme='<id>']` 选择器；颜色对比度与新增主题的完整流程见 skill `astroman-appearance`

## 页面骨架

- 页面统一 `PageHeader` 提供标题/副标题/右侧操作
- 长内容分栏用 `a-row`/`a-col` 或自定义 flex，滚动区固定在内容列，不要让整页滚动
- 表格、表单、弹窗全部用 Ant Design Vue 组件，反馈用 `message` / `Modal`（`import { message, Modal } from 'ant-design-vue'`）
- 确认类操作用 `Modal.confirm`，危险操作文案写清后果（例如"将删除该项目的所有接口与历史"）

## 分栏（`components/split/`）

双栏页面一律用 `SplitPane`，不要再用 `a-row` / grid 手搓——v1.1.5 起它同时支持左右与上下两种方向：

| 文件 | 职责 |
|---|---|
| `SplitPane.vue` | 渲染两侧内容 + 分隔条，`direction` 为 `horizontal`（左右）或 `vertical`（上下） |
| `PaneDivider.vue` | 分隔条：拖动、双击复位、悬停高亮，键盘可调 |
| `SplitLayoutSwitch.vue` | 方向切换按钮，自绘 SVG 方框图标（竖线切 = 左右，横线切 = 上下）；**不要用 antd 的 `ColumnWidth` / `ColumnHeight`**，形似字母 H / I 会被误读成「切换」 |

约定（改分栏前先看这几条，都是踩过的坑）：

- 比例状态走 `composables/useSplitLayout.ts`，键名 `xxx:dir` 与 `xxx:ratio:vertical` / `xxx:ratio:horizontal`；**比例按方向分键**，否则切方向时一半的宽度会被当成高度
- 默认 `vertical`（上下），因为多数工具左右两栏都只需要半屏宽
- 分栏容器**不要写死 `min-height` / `max-height`**：拖动后要么溢出要么撑不开。高度交给外层 `clamp()` 档位（左右 `clamp(560px, 76vh, 1080px)`、上下 `clamp(620px, 82vh, 1200px)`，`< 1200px` 用紧凑档），两侧内容用 `flex: 1` / `min-height: 0` 自适应
- 已接入：JSON 工作台、代码格式化、AES、Markdown、Diff、正则、MyBatis、图片 Base64 共 8 个工具，以及请求编辑器；新增双栏工具照着接，不要新写一套拖拽
- 视觉核验至少做「上下 → 左右 → 上下」三次切换，确认容器高度稳定且 `overflowX` 为 0

## 大文本渲染（`components/ResponseBody.vue` + `LargeTextViewer.vue`）

响应正文上限 5MB，整段塞进单个 `<pre>` 会让浏览器对全部字符做折行排版，主线程被占满（表现为打开响应即卡死几秒）。现在的分层：

| 阈值 | 行为 |
|---|---|
| `colorMax = 128KB` | 超过即关语法着色，只保留搜索标记（实测 128KB 着色约 0.13s、512KB 约 1.7s、1MB 约 6.7s） |
| `chunkMax = 1MB` | 超过改为分块渲染：首屏 96KB，之后每块 192KB，切点落在换行后；已渲染的块不再重排 |
| 超过 1MB | 关闭搜索（跨块匹配成本与收益不成比例），顶部标注体积与降级原因 |

- 分块逻辑在 `utils/textChunk.ts`（`DEFAULT_CHUNK_OPTIONS`），改阈值先看 `utils/__tests__/textChunk.spec.ts` 的边界用例
- 不要为了「体验一致」把阈值调高：着色成本随体积超线性增长，128KB 是按低端机实测定的

## 请求体体积

- 上限常量前后端各一份且**必须一致**：`src/utils/bodySize.ts` 的 `REQUEST_BODY_MAX_BYTES` 与 `src-tauri/src/commands/http.rs` 的 `MAX_REQUEST_BODY_BYTES`（32MB）
- 前端拦在触发前置脚本之前（脚本可能改大 body），后端兜住批量 / 套件 / AI / CLI 等不经过编辑器的入口
- multipart 的本地文件与 Binary 正文不计入请求体上限；单文件另有 2GB 上限（`MAX_UPLOAD_FILE_BYTES`），先读元数据再决定是否读入内存

## 代码编辑器（`components/CodeArea.vue`）

请求 Body、GraphQL、工具页共用这一个组件，编辑器的排版、着色、快捷键只在这里改。v1.1.1 起基于 CodeMirror 6，只渲染可视区域，不要再改回 textarea + 高亮层叠加。

| prop | 说明 |
|---|---|
| `v-model` | 文本内容 |
| `rows` / `fill` | 固定高度（`rows * 20 + 20`）或撑满父容器 |
| `readonly` / `placeholder` | 只读、占位文案 |
| `language` | `'plain' \| 'json'`，JSON 模式启用语法着色（请求编辑器只在 `form.bodyType === 'json'` 时传 `json`） |
| `searchable` | 显示查找替换入口（`Ctrl / ⌘ + F` 始终可用） |

实现要点：

- JSON 着色兼容注释与 `{{变量}}`：字符串内变量额外加底色（`cm-template-var`），所以带变量的 JSON 不会因为 `{{...}}` 打断着色
- 注释只用于编辑态：发送前由 Rust 的 `util::strip_json_comments` 剥离（见 `astroman-backend`），前端不负责删注释
- 格式化走 `utils/formatJson.ts`：`formatJsonAsync` 在 Worker 里解析，Worker 不可用时退回主线程；不要在大 Body 上直接 `JSON.parse`

## JSON 模板格式化（`utils/formatJson.ts`）

- `formatJsonTemplateAsync(text)` 专供请求 Body：先把 `{{...}}` 换成哨兵（字符串内只换文本、裸值补引号），格式化后按编号还原，`"ts": {{timestamp}}` 这类数值占位也能格式化且引号不变
- 请求编辑器「格式化 JSON」必须用它，不要换回 `formatJsonAsync`；普通 JSON 行为保持不变
- 用例在 `src/utils/__tests__/formatJson.spec.ts`（裸值 / 字符串内 / 数组 / 普通 JSON 四种）

## 数据池页（`views/DataPools.vue`）

- 左栏池列表 + 右栏条目：池类型（Token / 参数）、是否敏感、列定义、条目数与健康度都在池摘要里；新建池弹窗里先编辑列，再创建
- 动态列：表格列来自 `pool.columns`，数据取 `item.data`；不要再固定「值 / 账号」两列
- Excel：`下载 Excel 模板` 与 `导入 Excel` 只调 Rust 命令（`pool_excel_template` / `pool_excel_import`），前端只负责保存路径与文件选择，**不要在前端解析 xlsx**
- 改列弹窗提交差量 `{ from?, name }[]`，后端在事务里迁移；列名规则与 24 列上限由后端校验，前端只做即时提示
- 敏感池「查看明文」一次展示整行 `data`，30 秒后自动隐藏

## 压测绑定与集群页（`views/StressTest.vue` / `views/Cluster.vue`）

- 绑定行的列选择：`"*"` 表示全部列同名注入（隐藏目标输入），具体列默认把目标填成列名；同池多绑定只能选同一 mode
- 开跑前统一走 `utils/poolGate.ts::ensurePoolPreview`，预览失败或用户取消都要阻止启动
- 「本次变量」面板（v1.1.2）：池绑定区下方的折叠面板，数据来自 `api.stressVarSources(config)`（只读，不返回值、不拦启动）。每行按真实写法展示 `{{name}}` / `{{secret:name}}` / `{{os:name}}`，标注类型、来源与状态；`ok === false` 的排最前，有问题才自动展开，标题显示「N 个 · X 个待处理」。触发重算的时机：`request` 变化、`modalEnvId` 变化、池绑定变化（400ms 防抖），打开配置抽屉时也刷一次
- 节点区默认「机器卡片」视图（v1.1.5）：面板头 `a-segmented` 切「卡片 / 表格」，选择存 `localStorage.cluster.nodeView`；卡片是 `auto-fit minmax(318px, 1fr)` 栅格，顶部 2px 色条按角色 + 状态着色，指标格只放实时值，所有峰值统一收在底部 `.nc-peak` 一行灰度小字（`peakLineText()` / `peakLineTitle()`）；表格视图保留原 13 列，两者数据同源，别把卡片做成另一套数据
- 集群节点表有「兼容」列：`poolColumns` 与 `envBundle` 任意为假都是旧版；计划含自定义列绑定时禁用「下发配置」并给出原因
- 切片体量：会话快照的 `poolEstimate` / `poolSliceLimitBytes` 用来显示「数据池体量」提示，节点 `poolSliceBytes` 显示在能力列，≥80% 变黄
- 不要在集群页让用户给从节点单独配池或配刷新；池由主节点统一切片下发
- 「历史」tab（v1.1.2）：主节点看 `clusterJobsList` / `clusterJobDetail`（筛选、搜索、分页、详情抽屉里是 `ClusterWorkerRow`），从节点看 `clusterSlaveHistoryList`（「本机参与过的任务」，含整机 CPU / 内存 / 网卡峰值）。节点表里「引擎 CPU」与「整机 CPU」是两列，别合并
- 折线图节流：`liveBuffer` 是**非响应式**缓冲，按 `liveRefreshSec`（1 / 2 / 5 秒）刷到 `chartNodes`（`shallowRef`），`document.hidden` 时停摆，超过 60 点抽稀。不要改回每秒 push 响应式数组，主机侧会卡

## 工作台与数据大屏

v1.0.8 起大屏不在首页：`views/Dashboard.vue` 只剩总览 KPI、快捷入口、测试场景 / 批量运行介绍、产品介绍、引导与 FAQ；图表全部在 `components/DashboardMetrics.vue`，由 `views/History.vue`（历史记录）挂载。v1.0.9 首页新增「今日待办」卡片，只读 `notes_counts`，点击跳 `/notes`，不要在首页再拉全量待办。

- 图表**不依赖图表库**：柱高/宽度用峰值归一后的百分比，颜色只用 `--fm-*`（成功/警告/危险），空数据必须走 `.dash-empty` 文案而不是空坐标轴
- 两个页面都用 `Promise.allSettled` 拉数据，失败时保留空态、不阻断其他区块
- 新增/调整图表的完整约定（SQL 口径、补零、断点）见 skill `astroman-dashboard`

## 请求编辑器多标签（`views/RequestEditor.vue`）

一个组件复用于 `/request/:id`，切标签只换路由参数，**不会重建组件**，所有瞬时状态都得自己清理。

- 响应结果、断言、计时、运行时变量都要在 `loadRequest` 开始时 `resetResponseState()`；否则新标签会看到上一个接口的结果
- 连续快速切标签会并发触发加载，用 `loadToken` 只让最后一次落地；发送中的响应还要用 owner id 校验，归属不符直接丢弃
- `switchTab` 先 `flushSave()` 再 `router.push`，并用 `switchToken` 作废排队中的切换；路由已被其他入口带走时不要再跳
- 新响应或切换 Pretty / Raw 后把正文滚动回顶部（`watch([result, bodyView])`）
- 标签栏溢出时 `revealActiveTab()` 把当前标签 `scrollIntoView({ inline: 'nearest' })`
- 重命名后同步 `stores/tabs` 与快速打开下拉项，避免下拉里还是旧名字

### 顶栏与页签

- 顶栏是**图标按钮组**（历史 / 文档 / 代码 / Cookie / AI 发送选项 / 证书 / 删除），不要再加文字按钮；图标必须带 `title` 与 `aria-label`，证书关闭时用危险色底
- 「实际请求」页签展示的是 `utils/curl.ts` 里 `previewRequest()` 的结果：最终 URL、带来源标记（用户 / 认证 / 自动）的请求头、未解析变量告警。它与 cURL 导出共用同一份 `headerListAll`，改渲染口径要两边一起改
- Host / UA / Accept-Encoding / Content-Length 由底层 HTTP 客户端补齐，前端拿不到，页签底部必须保留这句说明，不要凭空编值
- 页签尽量细线化（`gap: 14px`、`padding: 8px 0`、`font-size: 12px`），无意义徽标（认证「无」、Body「none」、计数 0）直接隐藏，否则 1440px 下 6 个页签会被 antd 折叠进省略号
- 协议下拉最小宽度 96px、发送按钮定宽，否则 `HTTP/1.1` 会被截成 `HT…`

## 工具箱（`src/tools`）

92 个离线工具由 `src/tools/registry.ts` 的 `TOOLS` 数组声明，9 个分类来自 `TOOL_CATEGORIES`；页面是 `views/Tools.vue`，纯函数工具套 `components/tools/ToolWorkbench.vue`，复杂工具用独立组件懒加载。

- 工具的计算逻辑写在 `src/tools/<域>.ts`（`transforms` / `converters` / `crypto` / `encoding` / `netparse` / `datagen` …），**不要写进 `.vue`**，否则没法进 vitest
- 新增工具、字段契约与搜索别名规则见 skill `astroman-toolbox`

## 自动化测试页（`views/Automation.vue`）

套件列表 → 用例 / 运行记录 / 数据集 / 设置四个页签；运行前先调 `test_preflight` 展示阻断项，运行中拉进度、结束后跳报告。用例编辑器按 `kind`（`request` / `python` / `stress`）切换表单，压测用例可直接从压测计划导入。

- 写操作与起跑前都要二次确认；`test_run_start` 返回 `runId` 后轮询 `test_run_get`，不要阻塞等待整个套件跑完
- 数据模型、产物与 CLI 口径见 skill `astroman-automation`

## 抓包页（`views/Capture.vue`）

左右两栏：左侧按 host 分组的流量树，右侧详情（概览 / 请求 / 响应 / 原始）。工具栏是录制、清空、导出 HAR / ZIP、CA 状态、系统代理与设置抽屉；详情里的「重发」打开可编辑弹窗；域名分组右键可导出该分组 HAR / ZIP。

- 页面用 `import * as api from '@/api/capture'`，类型在 `src/types/capture.ts`，**不要**把这些命令再塞进 `api/index.ts`
- 流量与状态分别 1.5 秒轮询；CA 信任状态 10 秒轮询、已信任后停止，只在页面可见时刷
- 分组右键菜单用 `Teleport to="body"` 定位，点击窗口任意位置 / 失焦都要关闭
- 开始录制固定传 `{ port, bindAddr, sslHosts: [], ignorePatterns: [], captureStatic: true }`；新增设置项时同步 `CaptureConfig` 与 Rust handler
- 浏览器预览下 `invoke` 不可用，页面会显示「代理后端不可用」提示，属预期
- 拦截能力拆在 `components/CaptureInterceptDrawer.vue`（规则列表与开关）与 `CaptureBreakpointModal.vue`（命中后的改包）；断点弹窗要能被外部事件唤醒，关闭时必须调 `capture_breakpoint_release_all`，否则请求会挂到 120 秒超时
- 协议、证书、手机、重发与拦截规则见 skill `astroman-capture`

## 记事本页（`views/Notes.vue` + `views/QuickCapture.vue`）

三栏布局：左栏类型 + 标签筛选，中间卡片流，右侧 **50% 宽度**详情抽屉；新建与编辑都走抽屉，不要退回居中弹窗。

- 列表状态在 `stores/notes.ts`，筛选（scope / keyword / tag / kind）变化后重新 `notes_list`；长列表前端静态分页
- 完成 / 删除走页面内 undo 条，不弹二次确认；稍后 10 分钟调用 `notes_snooze` 后必须用返回值刷新抽屉里的 `snoozeUntil`，不能只改本地展示
- 标签颜色由 `utils/notesTags.ts` 按名称稳定派生，侧栏、列表、卡片共用；不要随机取色
- 附件预览用 `components/notes/NotesImageViewer.vue`；上传前 `utils/notesImage.ts` 压缩，原图以 base64 传后端去重
- 截图生成走 `components/notes/NotesImportDialog.vue`：只列 `supportsVision` 的服务商，识别失败要保留原图与 `rawText`
- 全局提醒卡片挂在 `NotesReminderHost.vue`，监听 `notes:reminder`；点击卡片发 `notes:open` 并定位到对应记录
- `QuickCapture.vue` 是独立窗口（hash `#/quick-capture`），不要塞进主窗口路由；解析逻辑在 `utils/notesQuickParse.ts` 纯函数里
- 数据语义与提醒规则见 skill `astroman-notes`

## 页面内 AI 入口（AiAssist）

给任意页面加「用 AI 分析当前内容」的能力，不需要新建页面或弹窗，统一复用全局抽屉：

1. 在 `src/utils/aiScenarios.ts` 加一个 `xxxPrompt(...)` 构造器，用 `detailBlock()` / `resultBlock()` 拼上下文，认证字段与密钥一律打码，长文本 `clip()`
2. 页面里 `const aiAssist = useAiAssistStore()`，用 `aiAssist.show({ title, prompt, hint })` 打开抽屉
3. 页头右侧加 `RobotOutlined` 按钮，与现有 `PageHeader` 的 `actions` 并列
4. 需要异步取数据再分析时，先 `await` 拿到数据再 `void aiAssist.show(...)`，失败用 `message.error`

约定：页面内 AI 是**只读建议**，`stores/aiAssist.ts` 的 `SYSTEM_PROMPT` 已禁止调用工具，所以不会触发写操作；抽屉的流式渲染由 store 统一处理，页面不要自己再写一份。上下文不再固定截 12k：store 按 `contextBudgetOf(model)` 分配，页面快照最多 70%，大报告用 `clipMiddle()` 保留头尾。

## 航仔挂件（`components/AstroBuddy.vue`）

`App.vue` 里全局挂载一次：`<AstroBuddy v-if="!route.path.startsWith('/ai')" @open="go('/ai')" />`，`/ai` 页自动隐藏。

- 形象用设计稿贴图 `src/assets/buddy.png`（288×483 透明底），显示 72×121；不要改回手绘 SVG，也不要在外面加白底卡片
- 拖拽走 Pointer Events + `setPointerCapture`：位移 ≤ 4px 仍算点击，否则拖拽；坐标存 `astroman.buddy.pos`，`clampPos`（`EDGE = 10`）保证拖不出窗口
- 甩动倾斜通过 CSS 变量 `--buddy-tilt` 传给 `.buddy-body`，**不要在外层元素写 `transform`**，否则会和浮动动画抢同一属性
- 小尺寸图标（侧栏 / 空状态 / AI 抽屉）用 `AstroBuddyIcon.vue`：`viewBox 0 0 24 24`、1em 线框，面罩用 `<mask>` 实心填充后挖掉双眼；`<svg>` 必须显式 `width/height: 1em`，否则 flex 容器里高度会塌成 0
- 动画都要在 `prefers-reduced-motion` 下关闭；细节与截图核验流程见 skill `astroman-appearance`

## 状态管理

| store | 职责 |
|---|---|
| `stores/project.ts` | 项目列表、当前项目、集合树、环境列表、激活环境；`refresh*` 方法负责重新拉取 |
| `stores/aiAssist.ts` | 页面内 AI 抽屉：`open/title/hint/messages/streaming`、`show()` 与 `ask()`，负责服务商选择与流式渲染 |
| `stores/tabs.ts` | 请求标签页（打开/切换/关闭），持久化到 localStorage |
| `stores/theme.ts` | 主题定义、`applyTheme`、初始化与持久化 |
| `stores/notes.ts` | 待办列表、筛选、计数、标签、设置缓存；监听 `notes:changed` 刷新 |

约定：**跨页面共享的状态才进 store**，单页面状态用 `ref`；调用后端后主动更新 store 而不是让多个页面各自缓存。

## API 层

- 全部走 `src/api/index.ts` 的封装，页面里不直接 `invoke`；抓包与记事本各自有 `src/api/capture.ts` / `src/api/notes.ts`
- 命名与 Rust 命令一一对应：`invoke<T>('request_save', { request })`
- 需要文件选择/保存时用 `@tauri-apps/plugin-dialog` 的 `open` / `save`，依赖 `capabilities/default.json` 中已有的 `dialog:default`

## 脚本引擎（`utils/scriptRunner.ts`）

执行顺序：环境脚本 → 前置脚本 → 发请求 → 后置脚本。作用域里注入：

- `request`：`method` / `url` / `headers` / `params` / `bodyType` / `body`（前置脚本可改）
- `response`：`status` / `statusText` / `durationMs` / `sizeBytes` / `headers` / `text()` / `json()`
- `env`：环境变量读写；`vars`：运行时变量（`vars.set` 的值优先级最高）
- `cipher`（`utils/cryptoScript.ts`）：AES/DES/3DES 加解密、`md5`/`sha1`/`sha256`、hmac、base64/hex
- `rsa.publicEncrypt`（`utils/rsa.ts`）
- `test(name, fn)` + `assert`：断言结果显示在响应区「测试」页
- `console.log/warn/error`：写入脚本日志

密钥长度约束：AES 16/24/32 字节、DES 8、3DES 24，IV 分别 16/8；十六进制或 Base64 密钥要传 `{ keyFormat: 'hex' | 'base64' }`。
插入示例代码由 `utils/scriptExamples.ts` 提供，改示例时同步更新 Dashboard 的「签名与加密」提示文案，避免两处说法不一致。

## 导出与文件

- 报告导出：`utils/reportExport.ts`（JSON 归档 / 可打印 HTML / 多工作表 Excel），写文件走 `report_write_file`
- 压测报告 QPS 口径：唯一入口是 `utils/reportAnalytics.ts` 的 `qpsOverview(detail, { rampUpSec })`，产出平均 / 峰值 / 最低 / 90% 分布 / 单并发 / 目标 QPS 与达成率；`ReportDetail.vue` 与 HTML / Excel 导出都调它。**不要在模板或导出函数里另算一遍**，否则同一个报告会出现两套数字。90% 分布 = 每秒采样 QPS 的 P10，最低 QPS 与它共用 `stableQpsValues()` 的稳态窗口（剔除升压段与末点），达成率与 SLA `minRps` 同口径用平均 QPS
- Excel 生成：`utils/xlsx.ts`（无第三方依赖，手写 OOXML）
- 其他工具：`utils/curl.ts`（cURL 解析/生成）、`utils/diff.ts`（响应对比）、`utils/mybatis.ts`、`utils/datetime.ts`

## 常见坑

1. **TDZ 白屏**：`App.vue` 中 `navs` 等常量必须声明在被 `watch`/`computed` 引用之前，否则 `setup()` 抛 `Cannot access 'navs' before initialization`，整页白屏且无报错遮罩
2. 浏览器里跑 `npm run dev` 时没有 Tauri IPC，`invoke` 报错属正常，验证功能必须 `npm run tauri dev`
3. 类型别手写：模型改动先看 `types/models.ts`，与 Rust 的 `models.rs` 保持一致
4. 图标/品牌资源：界面 logo 与侧栏标志统一用 `public/logo.png`，由 `AppLogo.vue` 输出 `<img>`（`size` 控制尺寸，圆角靠 CSS 裁）；`src-tauri/icons` 只保留打包/托盘这一套；`src/assets` 只放航仔贴图 `buddy.png`（走 vite 资源管线，构建后带 hash），不要再往里放 logo 副本
5. **`v-html` + scoped 样式**：`v-html` 生成的节点不带 `data-v-*`，`.tok-key { ... }` 这类选择器编译成 `.tok-key[data-v-x]` 会全部失效（现象：高亮层有 DOM 但完全没有颜色）。必须写成 `.highlight :deep(.tok-key)`；同理，任何 `v-html` 内容样式都要走 `:deep()`
6. **安装版看不到前端改动**：`tauri build` 会把 `dist/` 打进二进制，只改源码或只跑 `npm run build` 不会影响已安装的 exe；开发期用 `npm run tauri dev`（Vite 热更新），验证发布效果要重新打安装包
