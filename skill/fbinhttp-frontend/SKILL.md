---
name: fbinhttp-frontend
description: FbinHttp 前端开发规范（Vue 3 + Ant Design Vue + Pinia + 设计系统 + 脚本引擎/导出工具）。改页面、改样式主题、加组件、调脚本能力、改报告导出时使用。触发词：前端、Vue、页面、样式、主题、暗色、CSS 变量、脚本、加密、导出报告。
---

# 前端规范

入口 `src/main.ts`：`createPinia` → `router` → `app.use(Antd)`，样式顺序固定为 `ant-design-vue/dist/reset.css` 之后加载 `styles/main.css`。
启动前会执行 `applyPlatformAttributes()`（给 `<html>` 打平台标记）与 `initTheme()`（恢复主题）。

## 设计系统

- 所有颜色/圆角/阴影只用 `main.css` 里的 `--fm-*` 变量，**不要写死 hex**
- 主要 token：`--fm-primary` / `-dark` / `-soft`、`--fm-success|warning|danger|purple`（各有 `-soft`）、`--fm-bg`、`--fm-surface`、`--fm-surface-alt`、`--fm-border` / `-strong`、`--fm-text` / `-2` / `--fm-muted`、`--fm-radius-sm|`radius`|lg`、`--fm-shadow-sm|`shadow`|lg`、侧边栏一组 `--fm-sidebar-*`、代码区 `--fm-code-bg` / `--fm-gutter-bg` / `--fm-json-*`
- 主题：`stores/theme.ts` 定义 `ThemeId = 'ocean' | 'graphite' | 'forest' | 'midnight' | 'amber' | 'teal' | 'rose' | 'dusk'`（8 套，含 2 套深色），每套用同名变量覆盖；新增主题只加一个 `ThemeDefinition` 并追加到 `themes` 数组，设置页会自动渲染，不要在组件里判断主题名
- 深色主题靠 `data-theme`（`midnight` / `dusk`）+ `applyTheme()` 统一换变量，需要针对深色单独调整时用 `html[data-theme='midnight']` / `html[data-theme='dusk']` 选择器

## 页面骨架

- 页面统一 `PageHeader` 提供标题/副标题/右侧操作
- 长内容分栏用 `a-row`/`a-col` 或自定义 flex，滚动区固定在内容列，不要让整页滚动
- 表格、表单、弹窗全部用 Ant Design Vue 组件，反馈用 `message` / `Modal`（`import { message, Modal } from 'ant-design-vue'`）
- 确认类操作用 `Modal.confirm`，危险操作文案写清后果（例如"将删除该项目的所有接口与历史"）

## 首页数据大屏

`Dashboard.vue` 里的图表**不依赖图表库**：柱高/宽度用峰值归一后的百分比，堆叠用 `column-reverse`，颜色只用 `--fm-*`（成功/警告/危险），空数据必须走 `.dash-empty` 文案而不是空坐标轴。数据来自 `api.dashboardMetrics()`，页面用 `Promise.allSettled` 拉取，失败时保留空态、不阻断其他区块。

新增/调整图表的完整约定见 skill `fbinhttp-dashboard`。

## 状态管理

| store | 职责 |
|---|---|
| `stores/project.ts` | 项目列表、当前项目、集合树、环境列表、激活环境；`refresh*` 方法负责重新拉取 |
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
