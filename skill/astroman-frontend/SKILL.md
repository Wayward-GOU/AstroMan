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
- 代码/JSON 专用色（8 套主题都定义了同一批）：`--fm-json-key`（字段名）、`--fm-json-string`（字符串）、`--fm-json-number`（数字）、`--fm-json-bool`（布尔与 `{{变量}}`）、`--fm-json-punct`（括号、逗号、冒号）；不要在组件里为暗色主题单独写死颜色
- 主题：`stores/theme.ts` 定义 `ThemeId = 'ocean' | 'graphite' | 'forest' | 'midnight' | 'amber' | 'teal' | 'rose' | 'dusk'`（8 套，含 2 套深色），每套用同名变量覆盖；新增主题只加一个 `ThemeDefinition` 并追加到 `themes` 数组，设置页会自动渲染，不要在组件里判断主题名
- 深色主题靠 `data-theme`（`midnight` / `dusk`）+ `applyTheme()` 统一换变量，需要针对深色单独调整时用 `html[data-theme='midnight']` / `html[data-theme='dusk']` 选择器

## 页面骨架

- 页面统一 `PageHeader` 提供标题/副标题/右侧操作
- 长内容分栏用 `a-row`/`a-col` 或自定义 flex，滚动区固定在内容列，不要让整页滚动
- 表格、表单、弹窗全部用 Ant Design Vue 组件，反馈用 `message` / `Modal`（`import { message, Modal } from 'ant-design-vue'`）
- 确认类操作用 `Modal.confirm`，危险操作文案写清后果（例如"将删除该项目的所有接口与历史"）

## 代码编辑器（`components/CodeArea.vue`）

请求 Body、GraphQL、AES/JWT 等工具页共用这一个组件，编辑器的排版、着色、快捷键只在这里改。

| prop | 说明 |
|---|---|
| `v-model` | 文本内容 |
| `rows` / `fill` | 固定高度（`rows * 20 + 20`）或撑满父容器 |
| `readonly` / `placeholder` | 只读、占位文案 |
| `commentable` | 开启 `Ctrl / ⌘ + /` 行注释切换：按行加 `// `，整块已注释则取消；保留缩进、跳过空行、光标与选区跟随 |
| `language` | `'plain' \| 'json'`，JSON 模式启用语法着色（请求编辑器只在 `form.bodyType === 'json'` 时传 `json`） |

实现要点：

- 原生 textarea 不能局部着色：下层 `<pre class="highlight">` 渲染高亮 HTML，上层 textarea 文字透明（`.editor.overlay { color: transparent; caret-color: var(--fm-text) }`），只保留光标与选区
- 两层字体、字号、行高、padding 必须完全一致；`onScroll` 同时同步 gutter 的 `scrollTop` 与高亮层的 `scrollTop / scrollLeft`
- `highlightJson()` 顺序扫描字符串 → 行注释 / 块注释 → `{{变量}}` → 数字 → 标点；字符串优先处理，所以 `"https://a.com/x"` 里的 `//` 不会被当成注释
- 输入法组合期间（`compositionstart` / `compositionend`）临时去掉 overlay 类，否则中文拼音与候选字不可见
- 注释只用于编辑态：发送前由 Rust 的 `util::strip_json_comments` 剥离（见 `astroman-backend`），前端不负责删注释

## 首页数据大屏

`Dashboard.vue` 里的图表**不依赖图表库**：柱高/宽度用峰值归一后的百分比，堆叠用 `column-reverse`，颜色只用 `--fm-*`（成功/警告/危险），空数据必须走 `.dash-empty` 文案而不是空坐标轴。数据来自 `api.dashboardMetrics()`，页面用 `Promise.allSettled` 拉取，失败时保留空态、不阻断其他区块。

新增/调整图表的完整约定见 skill `astroman-dashboard`。

## 页面内 AI 入口（AiAssist）

给任意页面加「用 AI 分析当前内容」的能力，不需要新建页面或弹窗，统一复用全局抽屉：

1. 在 `src/utils/aiScenarios.ts` 加一个 `xxxPrompt(...)` 构造器，用 `detailBlock()` / `resultBlock()` 拼上下文，认证字段与密钥一律打码，长文本 `clip()`
2. 页面里 `const aiAssist = useAiAssistStore()`，用 `aiAssist.show({ title, prompt, hint })` 打开抽屉
3. 页头右侧加 `RobotOutlined` 按钮，与现有 `PageHeader` 的 `actions` 并列
4. 需要异步取数据再分析时，先 `await` 拿到数据再 `void aiAssist.show(...)`，失败用 `message.error`

约定：页面内 AI 是**只读建议**，`stores/aiAssist.ts` 的 `SYSTEM_PROMPT` 已禁止调用工具，所以不会触发写操作；抽屉的流式渲染由 store 统一处理，页面不要自己再写一份。上下文不再固定截 12k：store 按 `contextBudgetOf(model)` 分配，页面快照最多 70%，大报告用 `clipMiddle()` 保留头尾。

## 状态管理

| store | 职责 |
|---|---|
| `stores/project.ts` | 项目列表、当前项目、集合树、环境列表、激活环境；`refresh*` 方法负责重新拉取 |
| `stores/aiAssist.ts` | 页面内 AI 抽屉：`open/title/hint/messages/streaming`、`show()` 与 `ask()`，负责服务商选择与流式渲染 |
| `stores/tabs.ts` | 请求标签页（打开/切换/关闭），持久化到 localStorage |
| `stores/theme.ts` | 主题定义、`applyTheme`、初始化与持久化 |

约定：**跨页面共享的状态才进 store**，单页面状态用 `ref`；调用后端后主动更新 store 而不是让多个页面各自缓存。

## API 层

- 全部走 `src/api/index.ts` 的封装，页面里不直接 `invoke`
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
- Excel 生成：`utils/xlsx.ts`（无第三方依赖，手写 OOXML）
- 其他工具：`utils/curl.ts`（cURL 解析/生成）、`utils/diff.ts`（响应对比）、`utils/mybatis.ts`、`utils/datetime.ts`

## 常见坑

1. **TDZ 白屏**：`App.vue` 中 `navs` 等常量必须声明在被 `watch`/`computed` 引用之前，否则 `setup()` 抛 `Cannot access 'navs' before initialization`，整页白屏且无报错遮罩
2. 浏览器里跑 `npm run dev` 时没有 Tauri IPC，`invoke` 报错属正常，验证功能必须 `npm run tauri dev`
3. 类型别手写：模型改动先看 `types/models.ts`，与 Rust 的 `models.rs` 保持一致
4. 图标/品牌资源：界面 logo 与侧栏标志统一用 `public/logo.png`，由 `AppLogo.vue` 输出 `<img>`（`size` 控制尺寸，圆角靠 CSS 裁）；`src-tauri/icons` 只保留打包/托盘这一套，不要再往 `src/assets` 放重复 logo
5. **`v-html` + scoped 样式**：`v-html` 生成的节点不带 `data-v-*`，`.tok-key { ... }` 这类选择器编译成 `.tok-key[data-v-x]` 会全部失效（现象：高亮层有 DOM 但完全没有颜色）。必须写成 `.highlight :deep(.tok-key)`；同理，任何 `v-html` 内容样式都要走 `:deep()`
6. **安装版看不到前端改动**：`tauri build` 会把 `dist/` 打进二进制，只改源码或只跑 `npm run build` 不会影响已安装的 exe；开发期用 `npm run tauri dev`（Vite 热更新），验证发布效果要重新打安装包
