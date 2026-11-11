# AstroMan UI 重设计实施方案（开发执行手册）

**「深空钛」设计系统 · 从设计稿到可合并代码**

| 项目 | 内容 |
|---|---|
| 文档版本 | v3.0（二次重设计 · P0 已落地） |
| 日期 | 2026-11-10 |
| 目标代码库 | `D:\Software\ProjectFiles\AstroMan`（Tauri 2 + Vue 3.5 + Ant Design Vue 4） |
| 代码基线 | `main.css` 480 行 / `theme.ts` 1052 行 / 20 个视图 38,997 行 |
| 设计依据 | `astroman-ui-proposal/index.html`（图 01–05）、`tokens.css` |
| 读者 | 接手开发的前端工程师或 AI 编码助手 |
| 文档性质 | **执行手册**：按本文顺序操作即可产出可运行、可验收的成品 |

---

> **v2.1 修订说明（相对 v2.0，4 处会让执行卡死的硬伤）**
>
> 1. **令牌定义收敛为一处**：C0 需删除 `main.css` 的 `:root` 50 个声明，`tokens.css` 改为在 `main.css` **之后**引入。否则旧的 `--fm-radius: 8px` / 旧投影 / 旧方法色会覆盖新令牌，C1 做完圆角仍是 8px、卡片仍有投影。
> 2. **`App.vue` 取色器改为按明暗组兜底**：`--fm-font-sans` 与 `--fm-border-control` 都不在 `theme.vars` 里，直读会得到 `undefined`。新增 `c()` 取色器后，C1 不再依赖 C5 补键。
> 3. **硬编码门禁排除 `tokens.css`**：该文件自带 59 个 hex，算进基线会让 C0 验收当场 `hex 484 > 425` 失败。
> 4. **C4 内容与动作分离**：`steps` 的 `go` 是函数、`tips` 没有 `tag/group`，原抽取脚本产出的 `manual.ts` 无法通过 `tsc`，分组筛选也全为 0。
>
> 其余章节结构、设计令牌取值、视图改造清单均未改动。

> **v3.0 二次重设计（2026-11-10，视觉方向以此版为准）**
>
> v2.x 把 23 套主题统一成「同一组纸色/夜航底 + 各自强调色」，验收后判定「颜色主题像摆设、布局只是换色」。经三版方向稿评审，确认按下述方向重做；**功能、路由数量与业务流程全部不变，只换 UI 与布局**：
>
> - 外壳：双导航。保留现有 12 项侧栏，增加 44px 顶栏；项目/集合二级树放进页面内，不再挤进全局侧栏（P1）。
> - 主题：23 套全部重做。每套独立维护基色，必须同时改变画布、卡片、侧栏、描边与文字，不再只是换主色。
> - 分期：P0 主题引擎 → P1 外壳 → P2 工作台 → P3 请求/项目/集合 → P4 其余页面 → P5 视觉基线。v2.x 的 C4（使用说明页）/ C6（基线重录）不再单独推进，并入 P4 / P5。
>
> **P0 已完成（2026-11-10）**
>
> 1. 新增 `src/design/themes.ts`：23 套主题 × 13 个基色（画布 / 卡片 / 次级底 / 侧栏 / 侧栏文字 / 发丝线 / 强描边 / 正文 / 次级文字 / 弱化文字 / 主色 / 明暗 / 金铜文字），其余 25 个令牌由 `buildColorVars()` 派生；侧栏选中文字自动提亮到 ≥5.5:1。
> 2. `src/stores/theme.ts` 从 1,140 行手写 38 变量收敛为约 90 行，`ThemeId` 由基色表推导；主题顺序、localStorage 键、`applyTheme` 行为不变。
> 3. `src/design/tokens.ts` 移除废弃的 `paperColors` / `nightColors` / `makeVars`；两套基准色改由 `themes.ts` 派生。`App.vue`、`ReportDetail.vue` 只调整 import 来源。
> 4. `src/styles/tokens.css` 首帧兜底同步新宣纸值（深色侧栏 `#15181A`）。
> 5. 设置页主题预览改为迷你卡片：深色侧栏条 + 画布 + 卡片 + 主色圆点 + 两行文字，23 套差异一眼可见；未新增硬编码色值或圆角声明。
> 6. 对比度门禁改为 `scripts/check-contrast.ts`（`vite-node` 直接 import 基色表），18 对 × 23 套（含侧栏文字、侧栏选中文字），全部达标；`ui:check` 已切换，旧 `check-contrast.mjs` 删除。
> 7. 细节回归（2026-11-11）：设置页主题选中勾从预览右上角的绝对定位浮层移到主题名右侧，原位置会压在缩略图描边上且缺少底色承载；记事本页头操作顺序改为「搜索 → 截图生成 → 提醒 → 通知 → 设置 → 新建」，主操作靠右，与工具箱 / 项目页一致。两处只动模板与局部 CSS，没有新增硬编码色值、圆角或投影声明。
> 8. 侧栏悬停修复（2026-11-11）：深色侧栏上的 `.nav-item:hover` 原先把文字和图标改成 `--fm-text-2` / `--fm-text`，悬停后接近黑色，看起来像「点上去变黑」。现在只换侧栏自身的色阶（背景 `--fm-sidebar-hover`、图标 `--fm-sidebar-active-text`），品牌字、品牌副标题与分区标签也改用 `--fm-sidebar-text` / `--fm-sidebar-icon`。折叠态悬停图标放大 1.22 倍，并在右侧弹出导航名称 tooltip（仅在折叠态启用），接近 Dock 的悬停反馈；展开态仍显示完整名称，不弹 tooltip。未新增硬编码色值、圆角或投影声明。
> 9. 响应 HTML 预览（2026-11-11）：Body 面板遇到 `text/html` / `application/xhtml+xml`，或正文以 `<!DOCTYPE html>` / `<html>` 开头时，原「Pretty / Raw」换成「源码 / 预览」两段（HTML 不做 JSON 格式化，Pretty 与 Raw 内容相同，因此不再保留无意义的 Pretty）。预览用 `sandbox=""` 的 `iframe srcdoc` 渲染，脚本、表单、弹窗与顶层跳转全部禁用，并带 `referrerpolicy="no-referrer"`；正文超过 2MB 时只给源码。预览是本地临时态，不写入 `workspace` 快照，`bodyView` 类型仍为 `pretty | raw`；切回「源码」或按 Ctrl+F 会立即卸载 iframe 并恢复响应搜索。切「源码」只改当前视图，不覆盖全局 Pretty/Raw 偏好（否则下一条 JSON 响应会被带成 Raw）；预览时 Body 页签角标改显示响应体大小，不再显示源码行数。Content-Type 通过 `headerOf` 取值，兼容大小写。iframe 铺满 Body 区，上方仅一条 11px 沙箱说明。打包 CSP 没有 `frame-src`，实测 Chrome 在 `default-src 'self'` / `frame-src 'none'` 下仍渲染 `srcdoc`，无需改 `src-tauri`；用 403 WAF 页面实测脚本未执行，paper / midnight 两套主题均可用。未新增硬编码色值、圆角或投影声明。
> 10. 中等宽度回归（2026-11-11，发版后）：`RequestEditor` 在 CSS 视口 1270–1450 之间（1906 物理宽窗口 @125%～150% 缩放）出现顶栏文字重叠、响应头右侧「对比」被裁。修复：顶栏 `.back-btn` 加 `flex-shrink:0 / white-space:nowrap`；`.name-input` 改为可收缩（`min-width:0 / max-width:100%`），`.crumb` 承接剩余宽度并裁切；响应头操作按钮收进 `.res-actions`，`max-width:1450px` 时响应头允许换行、操作行右对齐，同时隐藏项目面包屑；`max-width:1300px` 再隐藏自动保存文案。1450 以上保持单行原布局（1525 实测「对比」完整可见），业务逻辑与既有 28/32–34px 密度不变。已用 Playwright 在 1271 / 1366 / 1525 三档截图核对，均无重叠与裁切；`npm run ui:check` 基线未上升，`npm run build` 通过。
> 11. 压测报告跟随主题（2026-11-11）：`ReportDetail.vue` 原先按 §8.4 套了固定「夜航舱」深色作用域（嵌套 `a-config-provider` + `AntTheme.darkAlgorithm` + `data-palette="night"` + `nightVars`），导致任何主题下都显示黑色。报告页的卡片、表格、图表与状态色本来就全部使用 `--fm-*` 令牌，因此移除了嵌套 provider、`data-palette`、`:style` 绑定、抽屉的 `:root-style`，以及仅用于绑定夜间变量的 `.report-scope` 包装；页面回到与其他视图一致的 `.page`，背景由 `.main` 的 `--fm-bg` 提供。实测宣纸主题下页面为浅纸底、卡片米白、图表主色跟随 `--fm-primary`；夜航主题下恢复深色。`ui:check` 基线未上升。
>
> 验收：`npm run ui:check`（hex 234 / rgba 19 / radius 447 / shadow 51 / deep 110 未上升；23 套对比度通过）、`npm run build`、`npm run e2e:smoke`（6/6）通过。
>
> **P0 落地偏差**
>
> 1. 亮色主题的侧栏统一改为深色（新方向要求导航成为稳定锚点），这是 21 套旧主题最明显的变化；`Capture.vue` 里 5 处把 `--fm-sidebar*` 当普通行底色 / 悬停色的写法已改为 `--fm-surface-alt` 与 6% 文字混色，否则深色侧栏令牌会让抓包树出现暗底暗字。
> 2. 主题定义不再是逐套 38 行字面量，`theme.ts` 现在依赖 `themes.ts` 的派生结果；以后新增主题只改基色表一处。
> 3. `swatches` 顺序改为 `[画布, 卡片, 侧栏, 主色, 正文, 发丝线]`（原 `[bg, sidebar, primary, text]`），目前仅设置页使用。
> 4. `tokens.ts` 的 `primitive` / `shade` 保留为 L0 参考与工具，暂无调用方。
> 5. P1 及之后未开工；当前页面结构、密度（28px 行 / 32–34px 控件）、12 项导航与快捷键均未改动。

> **实施进度（2026-11-10）**
>
> - ✅ **C0 已完成**：`src/design/tokens.ts`、`src/design/validate.ts`、`src/styles/tokens.css` 已建；`main.ts` 在 `main.css` 之后引入；`main.css` 第 2–60 行 `:root`（50 个声明）已删除；三个门禁脚本已建，`package.json` 增加 `ui:check`。
> - ✅ **C1 已完成**：AntD 覆盖段、平台圆角、页面骨架、`App.vue` 的 27 组 `theme` 映射、`applyTheme` 的 `data-palette` 均已落地；`npm run build`、`npm run e2e:smoke` 通过。
> - ✅ **C2 已落文件**：`src/styles/components.css` 已建并在 `main.ts` 引入；视图尚未套用 `fm-*` 类名（随 C3），现阶段无视觉变化。
> - ✅ **C5 已完成（2026-11-10）**：`scripts/theme-migrate.mjs` 已建，21 套旧主题按 `PRIMARY_MAP` 重新生成 38 键（对比度全部 ≥4.5），新增 `paper`（宣纸，默认）/ `night`（夜航舱）两套；`ThemeId`、`themes` 首位、`loadThemeId()`/`findTheme()` 兜底均已改为 `paper`。**视图背景、卡片、控件、侧边栏现在统一为纸色/夜航舱底 + 各自强调色**，这是首个肉眼可见的整体换皮。
> - ✅ **C3-3 工作台已重做（2026-11-10）**：`Dashboard.vue` 按设计初稿图 01 重排首屏——4 宫格 KPI、最近项目表 + 今日待办双栏、4 个入口一行、十格上手路径、底部状态条；长教程区块默认折叠。`PageHeader.vue` 同步改为「等宽铜色 eyebrow / 28px 衬线标题 / 12px 副标题」。`npm run build`、`npm run ui:check`、`npm run e2e:smoke` 通过，硬编码 hex 425→380。
> - ✅ **C3 其余 19 个视图已完成（2026-11-10）**：按 C3-1～C3-4 四批清理完毕——统一语义令牌、2/3px 圆角、去装饰投影与渐变、等宽数字/标签；选中态统一为 `inset 2px 0 0 var(--fm-primary)`；`ReportDetail` 落地固定夜航舱局部作用域，`AiAssistant` 消息改为左侧 2px 关注线 + 等宽角色标签。`npm run ui:check`（hex 234、rgba 19、radius 447、shadow 51、deep 110，均已下调为新基线；宣纸/夜航舱对比度 24 对全 ✅）、`npm run build`、`npm run e2e:smoke`（6/6）通过。收尾又清掉 `History` 的 5 处回退色与 1 条中性灰进度槽，并把 Dashboard/Notes/Settings/Capture/RequestEditor 剩余字面圆角归入 2/3px 令牌（hex 239→234、rgba 20→19）。
> - 🚧 **C4 使用说明页 / C6 基线重录**：未开工。
>
> **落地偏差（相对本文）**
>
> 1. 补齐了 13 个旧视图已在用、但从未声明的兼容别名（`--fm-text-3/4`、`--fm-mono`、`--fm-bg-soft` 等），统一映射到语义令牌；其中 `RequestEditor` 的 `var(--fm-text-3)` 无 fallback，此前一直是失效状态。
> 2. 硬编码门禁除 `tokens.css` 外，也排除 `components.css`（同为集中定义层，只允许 `var(--fm-*)`），否则 C2 一新增文件 `radius` 就超基线。
> 3. C0 并非完全「无视觉变化」：`theme.vars` 不包含圆角与方法色，删掉 `:root` 后 `--fm-radius` 6/8/14→2/3/3、12 个方法色改为低饱和；阴影仍由 `theme.vars` 内联覆盖，未变。若需 C0 严格无变化，应先把旧圆角/方法色抄进 `tokens.css`，到 C1 再切新值。
> 4. 实测基线：C0 后 `hex 381`（删旧 `:root` 后低于 425）；C2 后 `radius 454`（C1 表格首末单元格圆角归零净 +1，已同步基线）。
> 5. C5 迁移脚本对 §6.3 的三处小改：① 只解析 `PRIMARY_MAP` 内的 21 套，新增的 `paper`/`night` 不在迁移范围，脚本可重复执行 `--check`；② 自检项补齐 `primary/bg`、`brass-text/bg`、`border-control/surface(≥1.5)`、`border/surface(≥1.3)`，与 `check-contrast.mjs` 同口径；③ `swatches` 改为从新色组派生 `[bg, sidebar, primary, text]`，否则设置页预览仍是旧配色。另：`ocean` 描述由「默认 · 鲜明现代」改为「亮色 · 鲜明现代」。
> 6. 视觉基线（`e2e/visual` 9 张）仍是换皮前版本，现在**按设计会失败**，待 C6 统一重录；`e2e:smoke` 仍通过。
> 7. 工作台为读取「最近接口 / 接口数 / 今日请求 / 系统代理 / 当前模型」新增了 3 个只读调用：`request_options`（仅最近 5 个项目）、`capture_status`、`ai_provider_list`；未改数据层、IPC 与已有调用参数。
> 8. `PageHeader.vue` 是全站共享组件，这次改动会同时影响其余 19 个页面：标题从 17px 无衬线改为 28px 衬线、eyebrow 改为等宽铜色、头部不再带底部分隔线，操作区改为与标题底对齐。属于 C3 的预期全局收益，但需在 C3 逐页回归时确认窄宽度换行。
> 9. 设计初稿的 48px 全局顶栏（面包屑 / 环境胶囊 / Ctrl K 搜索）未实现：本方案约束 `App.vue` 只改主题与导航，顶栏属于跨页面布局，留待单独决策；工作台首屏因此比初稿少一条顶栏。
> 10. 工作台状态条按页内 `position: sticky` 实现，不是全局固定底栏；「本次运行时长 / 内存」没有现成只读接口，改为显示版本号，避免为了读数新增监控采样。
> 11. 上手路径的完成度由现有统计推断（项目 / 环境 / 接口 / 请求历史 / 压测报告 5 个前置步骤，按顺序取连续完成数）；抓包、航仔等没有持久化“已使用”标记，不虚报完成。长教程区块默认折叠，由路径条右侧「打开使用说明」展开并滚动定位。
> 12. C3 按视图实际结构选择性套用 `.fm-card`（Dashboard/ReportDetail/StressTest/SystemMonitor 等大量使用，其余保留原 `.card/.panel` 类名但同口径：1px 描边、token 圆角、无投影）。没有为了统一类名去重写模板结构。
> 13. `QuickCapture` 实际窗口是 520×200（`src-tauri/src/notes/quick.rs`），不是 §8.1 写的 240×140；本次不改窗口尺寸与 Rust 侧，只清理圆角/投影/边框。
> 14. `Runner` 实际是「01 执行配置 / 02 选择接口 / 03 …」编号面板，不是步骤向导；保持现有三段结构，仅 token 化颜色与圆角。
> 15. `Capture` 实际是左右两栏可拖拽分栏（抓包流 + 详情），不是 §8.5 的 260px/1fr/380px 三栏；保留分栏交互，仅把流表行高压到 28px 并 token 化。
> 16. `ReportDetail` 没有复制整套 AntD token，而是嵌套深色 `a-config-provider` + `toCssVars(nightColors)` 本地重绑 38 个 CSS 变量；另外补了 `colorBgContainerDisabled` / `colorTextDisabled` / `colorFill*`，修复空态下禁用按钮发白。手写 SVG 全部改读 `var(--fm-*)`。请求详情抽屉会 teleport 出 `.report-scope`，必须再给 `a-drawer` 传 `:root-style="nightVars"` 让内容读到夜航变量，并在嵌套主题里补 `components.Drawer.colorBgElevated`（根配置按当前主题写过这一项，否则抽屉会出现纸底夜字）；两者缺一都会造成低对比。**2026-11-11 已按用户反馈移除整套固定深色作用域，报告页改为跟随当前主题，见 P0 第 10 条。**
> 17. `AiAssistant` 去掉用户气泡底色/边框，改为「助手左侧 2px 主色线、用户右侧 2px 强描边线 + 等宽小标签」；保留 26px 头像（纯色块 + token 圆角）以免丢失角色识别；工具卡用左侧 2px 铜线区分。用户消息不再有独立卡片背景，这是 §8.6 的刻意效果。
> 18. `StressTest` 保持现有字段行网格，未改造成 KPI 条；只把图表系列、进度条、状态点颜色与圆角 token 化。

## 阅读与执行约定

本文不是设计说明，是施工图。执行者请严格按以下约定工作：

**一、本文所有「行号」基于 git 提交 `v1.1.0` 的当前工作区**。若行号已漂移，用给出的 `grep` 关键串定位，不要按行号硬改。

**二、改造分为 7 个提交（C0–C6）**，每个提交都可独立构建、独立回滚。不要把所有改动堆成一个提交——这是本方案唯一强制的流程要求。「§12 提交级任务清单」给出每个提交的文件清单、命令、验收与回滚方式。

**三、禁止事项（硬约束）**

| 禁止 | 原因 |
|---|---|
| 改路由表条数与路径 | 20 个视图是功能基线，少一个即违约 |
| 改 `src/api/`、`src-tauri/`、`src/stores/`（除 `theme.ts`） | 数据层与 IPC 零改动 |
| 改视图的业务逻辑（`setup` 内的函数体、`api.*` 调用、`v-if` 条件） | 只允许改 `<style>` 与模板的 class/结构包装 |
| 删除任何 `navs` 项、按钮、字段、Tab、快捷键 | 功能零删减 |
| 删除 21 套主题中任意一套 | 只允许新增与重映射 |
| 在 `<style>` 中新增 `#hex`、`rgba()`、字面量圆角 | 由 `check-hardcoded-color.mjs` 拦截 |
| 新增 `!important` | 现有 `!important` 是历史包袱，只能减不能增 |

**四、允许改动的范围**

`<style>` 块全部、模板中的 class 名与包裹元素、`src/styles/*.css`、`src/design/*`、`src/stores/theme.ts` 的 vars 值、`src/App.vue` 的 `theme` computed 与 `navs`、`src/router/index.ts`（仅新增 1 条）、`src/data/manual.ts`（新增）。

**五、可复制的验收命令**（每阶段必跑）

```bash
cd /d/Software/ProjectFiles/AstroMan
npm run build                 # vue-tsc --noEmit && vite build
npm test                      # vitest run
npm run e2e:smoke             # 关键路径
npm run e2e:visual            # 视觉基线
node scripts/check-hardcoded-color.mjs   # 硬编码门禁（本方案新增）
node scripts/check-contrast.mjs          # 对比度门禁（本方案新增）
```

---

## 01 代码基线：改造前必须知道的 12 个事实

下表每一项都经过实际读取确认，括号内为定位串。**这些事实决定了改造顺序，跳过会返工。**

| # | 事实 | 位置 | 对改造的影响 |
|---|---|---|---|
| 1 | `main.css` 的 `:root` 声明 **50 个**变量：38 个（颜色 + 圆角 + 投影）+ 12 个 HTTP 方法色，其中 `method-*` 12 个**不在** `theme.vars` 内 | `src/styles/main.css` 第 2–60 行 `:root {` | 方法色跨主题恒定，切主题不会丢；**但这 50 个声明必须在 C0 整块删除**，否则与 `tokens.css` 同名打架（见 §4.2） |
| 2 | `theme.ts` **1052 行**，21 套 × 每套 **35 个**键，无任何生成逻辑 | `src/stores/theme.ts` `const ocean` | 35 键必须逐套补全，少一键则 `applyTheme` 后该键回退到 `:root` 值，深色下会露馅 |
| 3 | `applyTheme()` 用 `root.style.setProperty()` **内联**写入，优先级高于 `:root` | `theme.ts` `export function applyTheme` | 颜色只能走运行时；CSS 里写同名的颜色会被覆盖，白改 |
| 4 | `applyTheme()` 已设置 `colorScheme`，但**未设置** `dataset.palette` | 同上 | 本次补 `dataset.palette`，供局部深色作用域使用 |
| 5 | `main.css` 第 **265–395 行**有 **28 条** AntD 全局覆盖，大量 `!important` | `.ant-btn {` 起 | 这是主战场的**主入口**，改这里比改 20 个视图的 `:deep` 省 80% 工作量 |
| 6 | 平台差异段又写死一遍圆角：`.ant-btn` mac 7 / win 5，`.ant-modal-content` mac 14 / win 8，`.page` mac 内边距 20 24 30 | `html[data-platform='macos'] .ant-btn` | **圆角改不动就是因为这里**。必须与 §05.1 同步改，否则 mac 上按钮仍是 7px |
| 7 | `App.vue` 的 `theme` computed 只映射 **13 个** token（另加 `fontSize` / `fontFamily` 两个非颜色项）；`colorBorder` 接的是 `--fm-border`（发丝线 `#E6E9F2`），而它语义上是**控件描边** | `App.vue` 第 464 行 `const theme = computed` | 输入框边界发虚的根因。必须拆出 `--fm-border-control`，且它当时**不在** `theme.vars` 里，取值要走兜底（见 §5.4） |
| 8 | `controlHeight` 为 `isMac ? 32 : 34`；`borderRadius` 为 `isMac ? 7 : 6` | 同上 | 高度本次不动（P2 再改），圆角本次统一到 3 |
| 9 | `navs` 共 **12 项**，分 4 组：调试 / 测试 / 智能 / 系统 | `App.vue` `const navs: NavItem[]` | 新增「使用说明」→ 13 项，插在「系统」组 `settings` 之前 |
| 10 | 路由 **20 条 + 1 条 catch-all**；`/quick-capture` 带 `meta: { bare: true }` 走裸布局 | `src/router/index.ts` | 新增 `/manual` 时必须**不加** `bare`，否则侧栏消失 |
| 11 | `Dashboard.vue` 教程数据在 `const steps = computed`（430 行）、`const tips = [`（503 行）、`const faqs = [`（596 行）；引导模板在 **1300–1360 行** | `grep -n "const steps = computed"` | 抽取数据源时先备份，用脚本比对条数（10 / 18 / 22） |
| 12 | `main.ts` 已在 `app.mount()` **之前**调用 `initTheme()` | `src/main.ts` 第 15–16 行 | 首帧无闪烁已成立，不要调整顺序；新增样式表 `import` 放在 `main.css` 之后 |

### 1.1 现状色值锚点（改造前先跑一遍，作为 diff 基线）

```bash
cd /d/Software/ProjectFiles/AstroMan
echo "hex:    $(grep -roh '#[0-9a-fA-F]\{3,8\}\b' src/views src/components src/App.vue | wc -l)"
echo "rgba:   $(grep -roh 'rgba(' src/views src/components src/App.vue | wc -l)"
echo "radius: $(grep -roh 'border-radius' src/views src/components src/styles | wc -l)"
echo "shadow: $(grep -roh 'box-shadow' src/views src/components src/styles | wc -l)"
echo "deep:   $(grep -roh ':deep(\.ant-' src/views src/components src/App.vue | wc -l)"
```

基线（2026-11-09 实测，统计范围含 `src/views` + `src/components` + `src/App.vue` + `src/styles`）：`hex 425 / rgba 89 / radius 453 / shadow 73 / deep 110`。

**门禁目标**：`hex ≤ 40 / rgba ≤ 10 / radius ≤ 120 / shadow ≤ 20 / deep ≤ 60`。只允许下降。

---

## 02 总体技术方案

### 2.1 三层令牌与单一事实源

| 层 | 内容 | 是否随主题变 | 存放 | 写入方式 |
|---|---|---|---|---|
| L0 原始值 | `#0F6B64`、`3px`、`16px` | 否 | `src/design/tokens.ts` | 仅 TypeScript 常量，不导出 CSS |
| L1 语义令牌 | `--fm-primary`、`--fm-bg` | **是**，23 套各一组 | `theme.ts` 的 `vars` | 运行时内联（路径 B） |
| L1s 静态令牌 | `--fm-radius`、`--fm-gap-*`、`--fm-font-*` | 否 | `tokens.css` 的 `:root` | CSS 静态（路径 A） |
| L2 组件层 | `.fm-btn`、`.fm-panel` | 否 | `components.css` | 只允许 `var(--fm-*)` |

**两条写入路径的硬规则**（重复强调，因为这是最容易返工的地方）：

```
颜色类（35 键）  → 只写进 theme.vars（路径 B）。写进 tokens.css 无效，会被内联覆盖。
形态类（radius/gap/font/motion） → 只写进 tokens.css（路径 A）。写进 theme.vars 会导致 23 套各写一遍，必然漂移。
新增令牌 → 先判断属于哪类，再决定写哪里。新增颜色若跨主题恒定（如 method-*），写 tokens.css 的 :root 即可。
```

### 2.2 为什么必须先改 `main.css` 再改视图

`main.css` 第 265–395 行的 28 条规则用了 `!important`，权重高于视图内任何 `:deep()`。如果先改 20 个视图、最后才动 `main.css`，会出现「所有视图改完，样式没变」的假象。**C1 阶段必须先把这 28 条改到位。**

### 2.3 执行顺序（依赖关系）

```
C0 令牌文件与门禁 + 删除 main.css 的 :root 50 个声明（无视觉变化，必须最先合）
 └ C1 全局样式 + AntD token 接管（一次改完，肉眼可见 80% 变化）
    ├ C2 组件层 components.css（面板/按钮/表格/侧栏/刻度）
    │   └ C4 使用说明页 + 上手路径条（依赖 C2）
    ├ C3 视图逐个清理硬编码（20 个，可分批合）
    └ C5 主题重映射 21 套 + 新增 2 套（不依赖 C1：C1 的取色器已按明暗组兜底）
       └ C6 视觉基线重录 + 验收
```

C1 与 C5 是**解耦**的：C1 只要求 `tokens.ts` 里存在 `paperColors` / `nightColors`（C0 就有），通过 §5.4 的 `c()` 取色器回退，不必等 21 套旧主题补齐新键。C5 只是把旧主题从"兜底值"换成"各自映射值"。

---

## 03 设计令牌全表（可直接赋值）

### 3.1 亮色「宣纸 paper」（默认主题）

```css
:root {
  /* 主色与语义色 */
  --fm-primary: #0F6B64;
  --fm-primary-dark: #0B524D;
  --fm-primary-soft: #E4EDEA;
  --fm-success: #2A7253;
  --fm-success-soft: #E7F0EB;
  --fm-warning: #8F6118;
  --fm-warning-soft: #F6EEE0;
  --fm-danger: #A83A2C;
  --fm-danger-soft: #F6E7E4;
  --fm-purple: #6A4A87;
  --fm-purple-soft: #EFEAF4;

  /* 底色与描边 */
  --fm-bg: #F3F1EC;
  --fm-surface: #FBFAF7;
  --fm-surface-alt: #F0EEE8;
  --fm-border: #DBD6CB;          /* 发丝线：只做分隔 */
  --fm-border-strong: #C9C2B5;
  --fm-border-control: #C9C2B5;  /* 新增：控件描边 */

  /* 文字 */
  --fm-text: #14181B;
  --fm-text-2: #39424A;
  --fm-muted: #616C74;           /* AA 校正：原 #6C777E 为 4.40 */

  /* 侧栏 */
  --fm-sidebar: #F7F5F0;
  --fm-sidebar-hover: #EFEDE6;
  --fm-sidebar-active: #E4EDEA;
  --fm-sidebar-text: #4E585F;
  --fm-sidebar-icon: #6C777E;
  --fm-sidebar-active-text: #0F6B64;

  /* 代码区 */
  --fm-code-bg: #FBFAF7;
  --fm-gutter-bg: #F0EEE8;
  --fm-json-key: #2B5D8C;
  --fm-json-string: #2A7253;
  --fm-json-number: #8F6118;
  --fm-json-bool: #6A4A87;
  --fm-json-punct: #8B959B;

  /* 形态：零投影，只留模态一处 */
  --fm-shadow-sm: none;
  --fm-shadow: none;
  --fm-shadow-lg: 0 18px 48px rgba(20, 24, 27, 0.16);

  /* 国之重器：青铜金只做编号与线框 */
  --fm-brass: #A97C3F;
  --fm-brass-text: #8A6530;
}
```

### 3.2 深色「夜航舱 night」

```css
:root {
  --fm-primary: #56B9AC;
  --fm-primary-dark: #6ECBBE;
  --fm-primary-soft: #16302E;
  --fm-success: #7FB79A;
  --fm-success-soft: #16241D;
  --fm-warning: #C9A063;
  --fm-warning-soft: #2A2317;
  --fm-danger: #D0705F;
  --fm-danger-soft: #2E1C19;
  --fm-purple: #B49FD0;
  --fm-purple-soft: #232030;

  --fm-bg: #0E1215;
  --fm-surface: #12171B;
  --fm-surface-alt: #171D22;
  --fm-border: #2C353D;
  --fm-border-strong: #35404A;
  --fm-border-control: #35404A;

  --fm-text: #E9E6E0;
  --fm-text-2: #B9BFC3;
  --fm-muted: #7E888E;

  --fm-sidebar: #11161A;
  --fm-sidebar-hover: #1A2126;
  --fm-sidebar-active: #1D272D;
  --fm-sidebar-text: #98A2A8;
  --fm-sidebar-icon: #7E888E;
  --fm-sidebar-active-text: #56B9AC;

  --fm-code-bg: #10151A;
  --fm-gutter-bg: #141A20;
  --fm-json-key: #7FA8C9;
  --fm-json-string: #7FB79A;
  --fm-json-number: #C9A063;
  --fm-json-bool: #B49FD0;
  --fm-json-punct: #6F7A80;

  --fm-shadow-sm: none;
  --fm-shadow: none;
  --fm-shadow-lg: 0 18px 48px rgba(0, 0, 0, 0.5);

  --fm-brass: #C9A063;
  --fm-brass-text: #C9A063;
}
```

### 3.3 HTTP 方法色（跨主题恒定，只降一档饱和）

这 12 个键**不进** `theme.vars`，只在 `tokens.css` 的 `:root` 声明一次。深色下通过 `[data-palette='night']` 局部覆写提亮。

```css
:root {
  --fm-method-get: #2A7253;      --fm-method-get-soft: #E7F0EB;
  --fm-method-post: #8A5E16;     --fm-method-post-soft: #F7EFE1;
  --fm-method-put: #2B5D8C;      --fm-method-put-soft: #E8EFF5;
  --fm-method-patch: #6A4A87;    --fm-method-patch-soft: #EFEAF4;
  --fm-method-delete: #A83A2C;   --fm-method-delete-soft: #F6E7E4;
  --fm-method-other: #5E6970;    --fm-method-other-soft: #EFF0F1;
}
[data-palette='night'] {
  --fm-method-get: #7FB79A;      --fm-method-get-soft: #16241D;
  --fm-method-post: #C9A063;     --fm-method-post-soft: #2A2317;
  --fm-method-put: #7FA8C9;      --fm-method-put-soft: #17242E;
  --fm-method-patch: #B49FD0;    --fm-method-patch-soft: #232030;
  --fm-method-delete: #D0705F;   --fm-method-delete-soft: #2E1C19;
  --fm-method-other: #9BA5AB;    --fm-method-other-soft: #1E2429;
}
```

> `main.css` 现有的 `.method-GET { color: var(--fm-method-get) }` 等类名保持不变，只换变量值。

### 3.4 静态令牌（形态、间距、字体、动效）

```css
:root {
  --fm-radius-sm: 2px;
  --fm-radius: 3px;
  --fm-radius-lg: 3px;

  --fm-gap-1: 4px;
  --fm-gap-2: 8px;
  --fm-gap-3: 12px;
  --fm-gap-4: 16px;
  --fm-gap-6: 24px;
  --fm-gap-8: 32px;

  --fm-font-serif: 'Source Han Serif SC', 'Noto Serif SC', 'Songti SC', SimSun, serif;
  --fm-font-sans: 'Inter', 'Segoe UI Variable Text', 'Microsoft YaHei UI', 'PingFang SC', sans-serif;
  --fm-font-mono: 'JetBrains Mono', 'Cascadia Mono', 'Cascadia Code', Consolas, monospace;

  --fm-dur-fast: 120ms;
  --fm-dur-mid: 180ms;
  --fm-dur-slow: 240ms;
  --fm-ease: cubic-bezier(0.2, 0.8, 0.2, 1);
}
```

### 3.5 字号与行高（写进 `components.css`，不设令牌，避免过度抽象）

| 用途 | 值 | 字族 |
|---|---|---|
| 页面标题 `h1` | 28px / 1.25 / 600 / `letter-spacing .04em` | 衬线 |
| 区块标题 `h2` | 16px / 1.4 / 600 | 无衬线 |
| 面板标题 | 12.5px / 1.4 / 600 | 无衬线 |
| 正文 | 12.5px / 1.7 / 400 | 无衬线 |
| 表格正文 | 12px / 1.5 / 400 | 无衬线 |
| 眉标（全大写） | 9.5px / 1.2 / 400 / `letter-spacing .18em` | 等宽 |
| 数字读数 | 10–13px / 500 / `tabular-nums` | 等宽 |
| 代码块 | 11.5px / 1.72 | 等宽，行高必须与行号一致 |

### 3.6 对比度验收（实算，背景取最差档 `#F0EEE8`）

| 前景 | 背景 | 比值 | 判定 |
|---|---|---|---|
| `#14181B` text | `#F0EEE8` | 16.03 | AAA |
| `#39424A` text-2 | `#F0EEE8` | 9.19 | AAA |
| `#616C74` muted | `#F0EEE8` | 4.63 | AA |
| `#0F6B64` primary | `#F3F1EC` | 5.63 | AA |
| `#FBFAF7` on primary | `#0F6B64` | 6.08 | AA |
| `#8A6530` brass-text | `#F3F1EC` | 4.67 | AA |
| `#A83A2C` danger | `#F0EEE8` | 5.47 | AA |
| `#2A7253` success | `#F0EEE8` | 4.99 | AA |
| `#8F6118` warning | `#F0EEE8` | 4.65 | AA |
| `#E9E6E0` on `#12171B` | — | 14.48 | AAA |
| `#7E888E` on `#171D22` | — | 4.69 | AA |
| `#56B9AC` on `#12171B` | — | 7.67 | AA |
| ~~`#4D6BFE`~~ 旧主色 | `#FFFFFF` | **4.33** | ❌ 现状欠账 |
| ~~`#8B94AD`~~ 旧弱化 | `#FAFBFF` | **2.93** | ❌ 现状欠账 |

`--fm-border` 是**非文字**元素，只需可见（≥1.3 即可）；`--fm-border-control` 需 ≥1.5，实测 1.70。

---

## 04 新建设计层代码（C0 提交）

### 4.1 `src/design/tokens.ts`（完整文件，可直接创建）

这是全系统唯一的事实源。颜色在此定义，`theme.ts` 与 `tokens.css` 都由它派生。

```ts
/**
 * AstroMan 设计令牌 · 单一事实源
 *
 * 分层：
 *   L0 primitive —— 原始色值，不直接使用
 *   L1 semantic  —— 语义色，按亮/暗两组定义，写入 theme.vars
 *   L1s static   —— 形态/间距/字体，写入 tokens.css，不随主题变
 *
 * 规则：颜色只在 theme.vars（运行时内联）生效；形态只在 tokens.css 生效。
 */

/* ---------- L0 原始值 ---------- */
export const primitive = {
  ink: '#14181B',
  ink2: '#39424A',
  paper: '#F3F1EC',
  paperSurface: '#FBFAF7',
  paperAlt: '#F0EEE8',
  hairline: '#DBD6CB',
  hairlineStrong: '#C9C2B5',
  teal: '#0F6B64',
  tealDark: '#0B524D',
  tealSoft: '#E4EDEA',
  jade: '#2A7253',
  brass: '#A97C3F',
  brassText: '#8A6530',
  cinnabar: '#A83A2C',
  amberDeep: '#8F6118',
  violetDeep: '#6A4A87',
  nightBg: '#0E1215',
  nightSurface: '#12171B',
  nightAlt: '#171D22',
  nightLine: '#2C353D',
  nightLineStrong: '#35404A',
  nightInk: '#E9E6E0',
  nightInk2: '#B9BFC3',
  nightMuted: '#7E888E',
  nightTeal: '#56B9AC',
  nightJade: '#7FB79A',
  nightBrass: '#C9A063',
  nightCinnabar: '#D0705F',
  nightViolet: '#B49FD0',
} as const

/* ---------- 色彩键集合（38 个，含 3 个 shadow 与 brass 两键） ---------- */
export const COLOR_KEYS = [
  'primary', 'primary-dark', 'primary-soft',
  'success', 'success-soft',
  'warning', 'warning-soft',
  'danger', 'danger-soft',
  'purple', 'purple-soft',
  'bg', 'surface', 'surface-alt',
  'border', 'border-strong', 'border-control',
  'text', 'text-2', 'muted',
  'sidebar', 'sidebar-hover', 'sidebar-active',
  'sidebar-text', 'sidebar-icon', 'sidebar-active-text',
  'code-bg', 'gutter-bg',
  'json-key', 'json-string', 'json-number', 'json-bool', 'json-punct',
  'shadow-sm', 'shadow', 'shadow-lg',
  'brass', 'brass-text',
] as const

export type ColorKey = (typeof COLOR_KEYS)[number]
export type ColorVars = Record<ColorKey, string>

/* ---------- 色彩工具：只做混色与明度，不引第三方库 ---------- */
function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '')
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ]
}

function rgbToHex(r: number, g: number, b: number): string {
  const to = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0')
  return `#${to(r)}${to(g)}${to(b)}`.toUpperCase()
}

/** 以 t 的比例把 b 混入 a（t=0 得 a，t=1 得 b） */
export function mix(a: string, b: string, t: number): string {
  const [r1, g1, b1] = hexToRgb(a)
  const [r2, g2, b2] = hexToRgb(b)
  return rgbToHex(r1 + (r2 - r1) * t, g1 + (g2 - g1) * t, b1 + (b2 - b1) * t)
}

/** amount > 0 提亮，< 0 压暗 */
export function shade(hex: string, amount: number): string {
  return amount >= 0 ? mix(hex, '#FFFFFF', amount) : mix(hex, '#000000', -amount)
}

/* ---------- L1 语义色：亮 · 宣纸 ---------- */
export const paperColors: ColorVars = {
  'primary': primitive.teal,
  'primary-dark': primitive.tealDark,
  'primary-soft': primitive.tealSoft,
  'success': primitive.jade,
  'success-soft': '#E7F0EB',
  'warning': primitive.amberDeep,
  'warning-soft': '#F6EEE0',
  'danger': primitive.cinnabar,
  'danger-soft': '#F6E7E4',
  'purple': primitive.violetDeep,
  'purple-soft': '#EFEAF4',
  'bg': primitive.paper,
  'surface': primitive.paperSurface,
  'surface-alt': primitive.paperAlt,
  'border': primitive.hairline,
  'border-strong': primitive.hairlineStrong,
  'border-control': primitive.hairlineStrong,
  'text': primitive.ink,
  'text-2': primitive.ink2,
  'muted': '#616C74',
  'sidebar': '#F7F5F0',
  'sidebar-hover': '#EFEDE6',
  'sidebar-active': primitive.tealSoft,
  'sidebar-text': '#4E585F',
  'sidebar-icon': '#6C777E',
  'sidebar-active-text': primitive.teal,
  'code-bg': primitive.paperSurface,
  'gutter-bg': primitive.paperAlt,
  'json-key': '#2B5D8C',
  'json-string': primitive.jade,
  'json-number': primitive.amberDeep,
  'json-bool': primitive.violetDeep,
  'json-punct': '#8B959B',
  'shadow-sm': 'none',
  'shadow': 'none',
  'shadow-lg': '0 18px 48px rgba(20, 24, 27, 0.16)',
  'brass': primitive.brass,
  'brass-text': primitive.brassText,
}

/* ---------- L1 语义色：暗 · 夜航舱 ---------- */
export const nightColors: ColorVars = {
  'primary': primitive.nightTeal,
  'primary-dark': '#6ECBBE',
  'primary-soft': '#16302E',
  'success': primitive.nightJade,
  'success-soft': '#16241D',
  'warning': primitive.nightBrass,
  'warning-soft': '#2A2317',
  'danger': primitive.nightCinnabar,
  'danger-soft': '#2E1C19',
  'purple': primitive.nightViolet,
  'purple-soft': '#232030',
  'bg': primitive.nightBg,
  'surface': primitive.nightSurface,
  'surface-alt': primitive.nightAlt,
  'border': primitive.nightLine,
  'border-strong': primitive.nightLineStrong,
  'border-control': primitive.nightLineStrong,
  'text': primitive.nightInk,
  'text-2': primitive.nightInk2,
  'muted': primitive.nightMuted,
  'sidebar': '#11161A',
  'sidebar-hover': '#1A2126',
  'sidebar-active': '#1D272D',
  'sidebar-text': '#98A2A8',
  'sidebar-icon': primitive.nightMuted,
  'sidebar-active-text': primitive.nightTeal,
  'code-bg': '#10151A',
  'gutter-bg': '#141A20',
  'json-key': '#7FA8C9',
  'json-string': primitive.nightJade,
  'json-number': primitive.nightBrass,
  'json-bool': primitive.nightViolet,
  'json-punct': '#6F7A80',
  'shadow-sm': 'none',
  'shadow': 'none',
  'shadow-lg': '0 18px 48px rgba(0, 0, 0, 0.5)',
  'brass': primitive.nightBrass,
  'brass-text': primitive.nightBrass,
}

/* ---------- 派生：给 21 套旧主题用 ---------- */
/**
 * 保色相、按亮/暗分组套用其余键。21 套主题重映射只走这一个入口。
 */
export function makeVars(
  primary: string,
  dark: boolean,
  overrides: Partial<ColorVars> = {},
): Record<string, string> {
  const base = dark ? nightColors : paperColors
  const vars: Record<string, string> = { ...base }

  vars['primary'] = primary
  vars['primary-dark'] = dark ? shade(primary, 0.12) : shade(primary, -0.14)
  vars['primary-soft'] = dark ? mix(primary, '#000000', 0.82) : mix(primary, base.surface, 0.88)
  vars['sidebar-active-text'] = primary

  Object.entries(overrides).forEach(([k, val]) => {
    if (val) vars[k] = val
  })

  const out: Record<string, string> = {}
  COLOR_KEYS.forEach((k) => {
    out[`--fm-${k}`] = vars[k]
  })
  return out
}

/** 原始色组 → 带 --fm- 前缀的对象（paper / night 两套新主题使用） */
export function toCssVars(colors: ColorVars): Record<string, string> {
  const out: Record<string, string> = {}
  COLOR_KEYS.forEach((k) => {
    out[`--fm-${k}`] = colors[k]
  })
  return out
}

/* ---------- L1s 静态令牌（形态/间距/字体/动效） ---------- */
export const staticTokens = {
  'radius-sm': '2px',
  'radius': '3px',
  'radius-lg': '3px',
  'gap-1': '4px',
  'gap-2': '8px',
  'gap-3': '12px',
  'gap-4': '16px',
  'gap-6': '24px',
  'gap-8': '32px',
  'font-serif': "'Source Han Serif SC', 'Noto Serif SC', 'Songti SC', SimSun, serif",
  'font-sans': "'Inter', 'Segoe UI Variable Text', 'Microsoft YaHei UI', 'PingFang SC', sans-serif",
  'font-mono': "'JetBrains Mono', 'Cascadia Mono', 'Consolas', monospace",
  'dur-fast': '120ms',
  'dur-mid': '180ms',
  'dur-slow': '240ms',
  'ease': 'cubic-bezier(0.2, 0.8, 0.2, 1)',
} as const
```

> `COLOR_KEYS` 是 **38 个键**，比旧 `theme.vars` 的 35 个多 3 个：`border-control`、`brass`、`brass-text`。这 3 个最终要补进 21 套旧主题（§06 的迁移脚本负责）。但在那之前，`App.vue` 的取色器会按当前明暗组回退到 `paperColors` / `nightColors`（见 §5.4 的 `c()`），所以 C1 不会因为缺键而拿到 `undefined`，也不会出现「深色主题上边框过亮」。

### 4.2 `src/styles/tokens.css`（完整文件）

```css
/* ============================================================
   静态令牌 + 首帧兜底
   颜色由 theme.ts 运行时写入（优先级更高）；此处仅为首帧兜底。
   不要在这里写只属于某套主题的颜色。
   ============================================================ */
:root {
  /* ---- L1 首帧兜底：宣纸 ---- */
  --fm-primary: #0F6B64;
  --fm-primary-dark: #0B524D;
  --fm-primary-soft: #E4EDEA;
  --fm-success: #2A7253;
  --fm-success-soft: #E7F0EB;
  --fm-warning: #8F6118;
  --fm-warning-soft: #F6EEE0;
  --fm-danger: #A83A2C;
  --fm-danger-soft: #F6E7E4;
  --fm-purple: #6A4A87;
  --fm-purple-soft: #EFEAF4;

  --fm-bg: #F3F1EC;
  --fm-surface: #FBFAF7;
  --fm-surface-alt: #F0EEE8;
  --fm-border: #DBD6CB;
  --fm-border-strong: #C9C2B5;
  --fm-border-control: #C9C2B5;

  --fm-text: #14181B;
  --fm-text-2: #39424A;
  --fm-muted: #616C74;

  --fm-sidebar: #F7F5F0;
  --fm-sidebar-hover: #EFEDE6;
  --fm-sidebar-active: #E4EDEA;
  --fm-sidebar-text: #4E585F;
  --fm-sidebar-icon: #6C777E;
  --fm-sidebar-active-text: #0F6B64;

  --fm-code-bg: #FBFAF7;
  --fm-gutter-bg: #F0EEE8;
  --fm-json-key: #2B5D8C;
  --fm-json-string: #2A7253;
  --fm-json-number: #8F6118;
  --fm-json-bool: #6A4A87;
  --fm-json-punct: #8B959B;

  --fm-shadow-sm: none;
  --fm-shadow: none;
  --fm-shadow-lg: 0 18px 48px rgba(20, 24, 27, 0.16);

  --fm-brass: #A97C3F;
  --fm-brass-text: #8A6530;

  /* ---- 形态 ---- */
  --fm-radius-sm: 2px;
  --fm-radius: 3px;
  --fm-radius-lg: 3px;
  --fm-gap-1: 4px;
  --fm-gap-2: 8px;
  --fm-gap-3: 12px;
  --fm-gap-4: 16px;
  --fm-gap-6: 24px;
  --fm-gap-8: 32px;

  /* ---- 字体 ---- */
  --fm-font-serif: 'Source Han Serif SC', 'Noto Serif SC', 'Songti SC', SimSun, serif;
  --fm-font-sans: 'Inter', 'Segoe UI Variable Text', 'Microsoft YaHei UI', 'PingFang SC', sans-serif;
  --fm-font-mono: 'JetBrains Mono', 'Cascadia Mono', 'Consolas', monospace;

  /* ---- 动效 ---- */
  --fm-dur-fast: 120ms;
  --fm-dur-mid: 180ms;
  --fm-dur-slow: 240ms;
  --fm-ease: cubic-bezier(0.2, 0.8, 0.2, 1);

  /* ---- HTTP 方法色：跨主题恒定，不进 theme.vars ---- */
  --fm-method-get: #2A7253;
  --fm-method-get-soft: #E7F0EB;
  --fm-method-post: #8A5E16;
  --fm-method-post-soft: #F7EFE1;
  --fm-method-put: #2B5D8C;
  --fm-method-put-soft: #E8EFF5;
  --fm-method-patch: #6A4A87;
  --fm-method-patch-soft: #EFEAF4;
  --fm-method-delete: #A83A2C;
  --fm-method-delete-soft: #F6E7E4;
  --fm-method-other: #5E6970;
  --fm-method-other-soft: #EFF0F1;
}

/* 深色作用域：全局深色主题与局部深色区块都生效 */
[data-palette='night'] {
  --fm-method-get: #7FB79A;
  --fm-method-get-soft: #16241D;
  --fm-method-post: #C9A063;
  --fm-method-post-soft: #2A2317;
  --fm-method-put: #7FA8C9;
  --fm-method-put-soft: #17242E;
  --fm-method-patch: #B49FD0;
  --fm-method-patch-soft: #232030;
  --fm-method-delete: #D0705F;
  --fm-method-delete-soft: #2E1C19;
  --fm-method-other: #9BA5AB;
  --fm-method-other-soft: #1E2429;
}
```

**`main.ts` 引入顺序**（`tokens.css` 必须在 `main.css` **之后**）：

```ts
import 'ant-design-vue/dist/reset.css'
import './styles/main.css'
import './styles/tokens.css'      // 新增：静态令牌 + 首帧兜底（必须在 main.css 之后）
import './styles/components.css'  // 新增（C2 阶段）
import './styles/print.css'       // 新增（C6 阶段）
```

**同时，C0 必须删除 `main.css` 第 2–60 行的整个 `:root { … }` 块（50 个声明）。**

这一步不是清理，是前提。旧的 `:root` 里仍有 `--fm-radius-sm: 6px`、`--fm-radius: 8px`、`--fm-radius-lg: 14px`、三档旧 `--fm-shadow`、12 个旧高饱和 `--fm-method-*`。CSS 同权重时后加载者生效，所以：

- 只改引入顺序、不删旧块 → `tokens.css` 放后面确实能赢，但同一个变量仍有两处定义，以后谁改哪一处都会漂移；
- 只删旧块、保持旧顺序 → 形态令牌能生效，但 `main.css` 里若有别处引用这些变量也仍然可用（`tokens.css` 已提供同名变量）。

两件事一起做才干净：**令牌定义只留 `tokens.css` 一处，运行时颜色由 `theme.ts` 内联覆盖。** 删完后跑 `npm run build` 与 §1.1 的统计，`radius` / `shadow` / `hex` 应同步下降——这是判断有没有删干净的最快方式。

### 4.3 `src/design/validate.ts`（dev 期自检）

```ts
import type { ThemeDefinition } from '@/stores/theme'
import { COLOR_KEYS } from './tokens'

/** 启动自检：每套主题必须包含 COLOR_KEYS 的全部键。只在 dev 生效。 */
export function assertThemesComplete(themes: ThemeDefinition[]) {
  if (!import.meta.env.DEV) return
  const required = COLOR_KEYS.map((k) => `--fm-${k}`)
  let fail = 0

  themes.forEach((t) => {
    const miss = required.filter((k) => !t.vars[k])
    if (miss.length) {
      fail++
      console.error(`[theme] ${t.id}(${t.name}) 缺少 ${miss.length} 个令牌: ${miss.join(', ')}`)
    }
  })

  if (fail === 0) console.info(`[theme] ${themes.length} 套主题令牌完整`)
}
```

调用位置（`main.ts`，`initTheme()` 之后）：

```ts
if (import.meta.env.DEV) {
  import('./design/validate').then((m) => m.assertThemesComplete(themes))
}
```

> 用动态 `import`，避免令牌工具函数被打进首屏。

---

## 05 全局样式改造（C1 提交）

### 5.1 `main.css` 第 265–395 行：28 条覆盖逐条改写

现状问题有三个：主按钮是**渐变 + 彩色光晕**（AI 味最重的一处）、圆角写死 6/7/10/12/14、过渡全靠 `box-shadow`。

**把 `/* ---------- Ant Design 统一微调 ---------- */` 起、到 `.ant-spin-container::after` 结束的整段替换为：**

```css
/* ---------- Ant Design 统一微调（「深空钛」） ---------- */
.ant-btn {
  font-weight: 500;
  box-shadow: none;
  transition:
    background-color var(--fm-dur-fast) var(--fm-ease),
    border-color var(--fm-dur-fast) var(--fm-ease),
    color var(--fm-dur-fast) var(--fm-ease);
}

/* 主按钮：纯色实底，去渐变去光晕。仪器上的按钮不该发光。 */
.ant-btn-primary.ant-btn-primary:not(.ant-btn-background-ghost):not(:disabled) {
  background: var(--fm-primary);
  box-shadow: none;
}
.ant-btn-primary.ant-btn-primary:not(.ant-btn-background-ghost):not(:disabled):hover,
.ant-btn-primary.ant-btn-primary:not(.ant-btn-background-ghost):not(:disabled):active {
  background: var(--fm-primary-dark);
}
.ant-btn-primary:not(.ant-btn-background-ghost):disabled {
  background: color-mix(in srgb, var(--fm-primary) 45%, transparent);
}

/* 默认按钮：悬停只加深描边并铺一层极浅底，不位移 */
.ant-btn-default:not(:disabled):hover {
  border-color: var(--fm-border-strong);
  color: var(--fm-text);
  background: var(--fm-surface-alt);
}

.ant-input,
.ant-input-affix-wrapper,
.ant-select .ant-select-selector {
  border-color: var(--fm-border-control);
  transition: border-color var(--fm-dur-fast) var(--fm-ease);
}

/* 聚焦：只改描边，去掉 AntD 的 3px 外发光环 */
.ant-input:focus,
.ant-input-focused,
.ant-input-affix-wrapper-focused,
.ant-select-focused .ant-select-selector {
  box-shadow: none;
  border-color: var(--fm-primary);
}

.ant-tabs-top > .ant-tabs-nav {
  margin-bottom: var(--fm-gap-2);
}
.ant-tabs-top > .ant-tabs-nav::before {
  border-bottom-color: var(--fm-border);
}
.ant-tabs-tab {
  padding: 9px 2px !important;
  font-size: 12.5px;
}
.ant-tabs-tab-active .ant-tabs-tab-btn {
  font-weight: 600;
}

.ant-table {
  font-size: 12px;
}
.ant-table-thead > tr > th {
  background: var(--fm-surface-alt) !important;
  color: var(--fm-muted) !important;
  font-family: var(--fm-font-mono);
  font-size: 9.5px !important;
  font-weight: 400 !important;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  border-bottom: 1px solid var(--fm-text) !important;
}
.ant-table-tbody > tr > td {
  border-bottom: 1px solid var(--fm-border) !important;
  padding-top: 8px !important;
  padding-bottom: 8px !important;
}
.ant-table-tbody > tr:hover > td {
  background: var(--fm-surface-alt) !important;
}
/* 去掉 AntD 首末单元格的圆角补偿 */
.ant-table-thead > tr > th:first-child,
.ant-table-thead > tr > th:last-child,
.ant-table-tbody > tr > td:first-child,
.ant-table-tbody > tr > td:last-child {
  border-radius: 0 !important;
}

.ant-modal-content {
  border-radius: var(--fm-radius-lg);
  padding: 20px 22px;
  box-shadow: var(--fm-shadow-lg);
}
.ant-modal-title {
  font-family: var(--fm-font-serif);
  font-size: 16px;
  font-weight: 600;
  letter-spacing: 0.02em;
}
.ant-modal-footer {
  margin-top: var(--fm-gap-4);
}

.ant-tree {
  background: transparent;
  font-size: 12.5px;
}
.ant-tree .ant-tree-node-content-wrapper {
  border-radius: var(--fm-radius-sm);
}

.ant-menu-light .ant-menu-item-selected {
  background: var(--fm-primary-soft);
  color: var(--fm-primary);
  font-weight: 600;
}

.ant-tag {
  border-radius: var(--fm-radius-sm);
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  line-height: 18px;
}
.ant-tooltip-inner {
  border-radius: var(--fm-radius-sm);
  font-size: 11.5px;
}
.ant-alert {
  border-radius: var(--fm-radius);
}
.ant-spin-container::after {
  border-radius: var(--fm-radius);
}

/* 弹层：去投影改描边，只有遮罩层本身需要脱离平面 */
.ant-dropdown-menu,
.ant-select-dropdown,
.ant-picker-dropdown .ant-picker-panel-container {
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  box-shadow: var(--fm-shadow-lg);
}
```

**同时删除**（被新段取代，留着会打架）：

| 原位置 | 原内容 | 处理 |
|---|---|---|
| 273–299 | 主按钮 `linear-gradient` + `color-mix` 彩色光晕共 4 条 | 整段删除 |
| 311–316 | `.ant-input:focus` 的 `0 0 0 3px` 外发光 | 由新段 `box-shadow: none` 取代 |

### 5.2 平台差异段：圆角必须同步改（否则 mac 上按钮还是 7px）

`main.css` 末尾的 `html[data-platform='macos']` / `[data-platform='windows']` 段：

```css
/* 改前 */
html[data-platform='macos'] .ant-btn { border-radius: 7px; }
html[data-platform='windows'] .ant-btn { border-radius: 5px; }
html[data-platform='macos'] .ant-modal-content { border-radius: 14px; padding: 20px 22px; }
html[data-platform='windows'] .ant-modal-content { border-radius: 8px; padding: 22px 24px; }

/* 改后：圆角统一，只保留内边距差异 */
html[data-platform='macos'] .ant-btn,
html[data-platform='windows'] .ant-btn {
  border-radius: var(--fm-radius);
}
html[data-platform='macos'] .ant-modal-content,
html[data-platform='windows'] .ant-modal-content {
  border-radius: var(--fm-radius-lg);
  padding: 20px 22px;
}
```

保留的部分：mac 的 `body` 字体栈（SF Pro 在 mac 上确实更合适）、mac 滚动条 10px / win 12px、mac 滚动条 `border-radius: 3px`。
删除的部分：`html[data-platform='macos'] .page { padding: 20px 24px 30px; }`——统一用 `22px 24px 30px`，不留两套。

### 5.3 `main.css` 其余段落调整

| 选择器 | 原值 | 新值 |
|---|---|---|
| `.page` | `padding: 18px 22px 30px`；`animation: fm-fade-in .24s ease` | `padding: 22px 24px 30px`；`animation: fm-fade-in 160ms ease` |
| `.page-title` | `22px / 700 / -0.2px` | `28px / 600 / 0.04em`，加 `font-family: var(--fm-font-serif)` |
| `.page-subtitle` | `13px`，`margin: 6px 0 0` | `12px`，`margin: 7px 0 0` |
| `.fm-card` | 含 `box-shadow: var(--fm-shadow)` | 删除该行 |
| `.section-head` | `margin: 30px 0 14px` | `margin: 24px 0 12px` |
| `.section-title` | `15px / 600 / -0.1px` | `16px / 600 / 0.02em` |
| `.section-desc` | `12px` | 加 `line-height: 1.7` |
| `.update-notes` | `border-radius: 8px` | `var(--fm-radius)` |
| `::-webkit-scrollbar-thumb` | `border-radius: 8px` | `var(--fm-radius)` |

`@keyframes fm-fade-in` 改为只动透明度（现状若含 `translateY`，删掉——仪器不该飘进来）：

```css
@keyframes fm-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
```

### 5.4 `App.vue` 的 `theme` computed：13 个 token → 完整映射

替换 `App.vue` 中整个 `const theme = computed(...)`（现状起于第 464 行）。顶部先补一行 import：

```ts
import { nightColors, paperColors, staticTokens, type ColorKey } from '@/design/tokens'
```

```ts
const theme = computed(() => {
  // 密度本次不动（保持 32/34），统一到 30 留到 P2 单独回归
  const controlHeight = isMac ? 32 : 34
  const current = currentTheme.value
  const vars = current.vars
  // 取色器：主题 vars 优先，缺键时按当前明暗组回退。
  // 不能直接写 vars['--fm-x']：C1 阶段 21 套旧主题还没有 border-control /
  // brass / brass-text，直接取值是 undefined，AntD 会退回默认算法色。
  // 也不能从 vars 取 font-sans / radius / gap —— 它们属于静态令牌，不在 vars 里。
  const base = current.dark ? nightColors : paperColors
  const c = (key: ColorKey) => vars[`--fm-${key}`] ?? base[key]

  return {
    algorithm: AntTheme[current.dark ? 'darkAlgorithm' : 'defaultAlgorithm'],
    token: {
      /* 语义色 */
      colorPrimary: c('primary'),
      colorInfo: c('primary'),
      colorSuccess: c('success'),
      colorWarning: c('warning'),
      colorError: c('danger'),
      colorLink: c('primary'),

      /* 文字三级 */
      colorText: c('text'),
      colorTextSecondary: c('text-2'),
      colorTextTertiary: c('muted'),
      colorTextQuaternary: c('muted'),
      colorTextPlaceholder: c('muted'),

      /* 描边：控件描边与发丝线必须分开 —— 本次最关键的一处修正 */
      colorBorder: c('border-control'),
      colorBorderSecondary: c('border'),
      colorSplit: c('border'),

      /* 底色 */
      colorBgLayout: c('bg'),
      colorBgContainer: c('surface'),
      colorBgElevated: c('surface'),
      colorFillQuaternary: c('surface-alt'),
      colorFillTertiary: c('surface-alt'),

      /* 形态 */
      borderRadius: 3,
      borderRadiusLG: 3,
      borderRadiusSM: 2,
      borderRadiusXS: 2,
      fontSize: 13,
      fontFamily: staticTokens['font-sans'],
      lineHeight: 1.7,

      /* 去投影 */
      boxShadow: 'none',
      boxShadowSecondary: 'none',
      boxShadowTertiary: 'none',

      /* 动效 */
      motionDurationFast: '0.12s',
      motionDurationMid: '0.18s',
      motionDurationSlow: '0.24s',
      motionEaseOut: 'cubic-bezier(0.2, 0.8, 0.2, 1)',

      wireframe: false,
    },
    components: {
      Button: {
        controlHeight,
        fontWeight: 500,
        primaryShadow: 'none',
        defaultShadow: 'none',
        dangerShadow: 'none',
        defaultBorderColor: c('border-control'),
        defaultColor: c('text'),
      },
      Input: {
        controlHeight,
        activeShadow: 'none',
        activeBorderColor: c('primary'),
        hoverBorderColor: c('border-strong'),
        paddingInline: 10,
      },
      InputNumber: { controlHeight, activeShadow: 'none' },
      Select: {
        controlHeight,
        optionSelectedBg: c('primary-soft'),
        optionSelectedColor: c('primary'),
        optionActiveBg: c('surface-alt'),
      },
      Table: {
        headerBg: c('surface-alt'),
        headerColor: c('muted'),
        headerSplitColor: 'transparent',
        borderColor: c('border'),
        rowHoverBg: c('surface-alt'),
        cellPaddingBlock: 8,
        cellPaddingInline: 16,
        footerBg: c('surface'),
      },
      Tabs: {
        itemSelectedColor: c('primary'),
        itemHoverColor: c('text'),
        inkBarColor: c('primary'),
        titleFontSize: 12.5,
        horizontalItemPadding: '9px 0',
        horizontalItemGutter: 24,
      },
      Collapse: {
        headerBg: 'transparent',
        contentBg: 'transparent',
        headerPadding: '0 16px',
      },
      Modal: {
        contentBg: c('surface'),
        headerBg: c('surface'),
        titleFontSize: 16,
      },
      Drawer: { colorBgElevated: c('surface') },
      Tag: { defaultBg: c('surface-alt'), defaultColor: c('muted'), borderRadiusSM: 2 },
      Tree: {
        nodeSelectedBg: c('primary-soft'),
        nodeHoverBg: c('surface-alt'),
        titleHeight: 28,
      },
      Menu: {
        itemSelectedBg: c('primary-soft'),
        itemSelectedColor: c('primary'),
        itemHoverBg: c('surface-alt'),
      },
      Dropdown: { paddingBlock: 4 },
      Tooltip: { colorBgSpotlight: c('text'), colorTextLightSolid: c('surface') },
      Message: { contentBg: c('surface') },
      Notification: { colorBgElevated: c('surface') },
      Pagination: { itemActiveBg: c('surface-alt') },
      Progress: { defaultColor: c('primary'), remainingColor: c('border') },
      Empty: { colorTextDescription: c('muted') },
      Steps: { colorPrimary: c('primary') },
      Alert: { defaultPadding: '8px 12px' },
      Descriptions: { labelBg: c('surface-alt') },
      Segmented: { itemSelectedBg: c('surface'), trackBg: c('surface-alt') },
      Switch: { colorPrimary: c('primary') },
      Checkbox: { colorPrimary: c('primary') },
      Radio: { colorPrimary: c('primary') },
      Slider: { trackBg: c('primary'), handleColor: c('primary') },
    },
  }
})
```

**逐条说明**

1. `colorBorder` 从 `--fm-border`（`#DBD6CB`，1.39:1）改为 `--fm-border-control`（`#C9C2B5`，1.70:1）。输入框、下拉、按钮描边立刻变清楚，这是「控件不像一个系统」的主因。C1 时旧主题还没有这个键，`c()` 会按 `dark` 回退到 `nightColors` / `paperColors`，所以**不必等 C5**。
2. `colorBorderSecondary` 与 `colorSplit` 接 `--fm-border`，表格分隔线仍然轻。
3. `borderRadius` 从 `isMac ? 7 : 6` 收为 `3`，`borderRadiusLG` 从 11/10 收为 `3`，`borderRadiusSM` 新增 `2`。
4. `boxShadow*` 全部 `none`，与 `--fm-shadow: none` 一致。
5. `components` 从 4 组扩到 **27 组**。以前只映射 Button / Input / Select / Table，这是「AntD 像另一套皮肤」的根因。
6. `controlHeight` 保持 `isMac ? 32 : 34`，本次不改。一次性降到 30 会让 20 个视图的行高、表头、工具条同时错位，回归量大于收益，留到 P2 单独做（风险见 §13 #4）。
7. `fontFamily` 从平台硬切改为 `staticTokens['font-sans']`。注意**不能**写成 `vars['--fm-font-sans']`：字体属于静态令牌（路径 A），`theme.vars` 里只有 35 个颜色键，那样取会得到 `undefined`，AntD 静默退回默认字体。原来 mac 用 SF Pro、win 用 Segoe UI，与自绘的 Inter 不一致，同一行出现两种字形。

### 5.5 `theme.ts` 的 `applyTheme`：补 `dataset.palette`

```ts
export function applyTheme(theme: ThemeDefinition) {
  const root = document.documentElement
  Object.entries(theme.vars).forEach(([key, value]) => root.style.setProperty(key, value))
  root.dataset.theme = theme.id
  // 新增：明暗标记与主题 id 解耦，供局部深色作用域与方法色使用
  root.dataset.palette = theme.dark ? 'night' : 'paper'
  root.style.colorScheme = theme.dark ? 'dark' : 'light'
}
```

> 为什么必须新增：`dataset.theme` 是主题 id（`ocean` / `midnight` / `paper`…），无法判断明暗。CSS 里写 `html[data-theme='night']` 是错的——`night` 是主题 id 不是明暗标记。所有需要判断明暗的样式一律用 `[data-palette='night']`。

---

## 06 主题体系：21 套保留 + 新增 2 套（C5 提交）

### 6.1 21 套主色映射结果（实算值，直接可用）

映射规则：**保色相 H → 饱和度 × 0.75 → 明度二分至对比度达标**（浅色 ≥5.0，深色 ≥6.0）。

| # | 主题 id | 名称 | 旧主色 | **新主色** | 对比度 |
|---|---|---|---|---|---|
| 01 | `ocean` | 极光蓝 | `#4d6bfe` | `#445FE3` | 5.01 |
| 02 | `graphite` | 石墨灰 | `#466a8f` | `#4F6A86` | 5.37 |
| 03 | `forest` | 森林绿 | `#2e9e6b` | `#327959` | 5.01 |
| 04 | `midnight` | 暗夜 | `#82a6f0` | `#90ABE2` | 7.82 |
| 05 | `amber` | 琥珀橙 | `#e0731c` | `#9D5C29` | 5.02 |
| 06 | `teal` | 青碧 | `#0d9488` | `#1B7870` | 5.04 |
| 07 | `rose` | 玫瑰 | `#e0447a` | `#BF3A68` | 5.01 |
| 08 | `clay` | 陶土红 | `#b8564a` | `#9E5950` | 5.03 |
| 09 | `dusk` | 暮色 | `#96adff` | `#A3B4F2` | 8.93 |
| 10 | `violet` | 紫罗兰 | `#7c5cf0` | `#7155D8` | 5.03 |
| 11 | `sky` | 天青 | `#0ea5e9` | `#1F749A` | 5.03 |
| 12 | `mint` | 薄荷 | `#10b981` | `#1C7A5B` | 5.04 |
| 13 | `coral` | 珊瑚 | `#f2714d` | `#B94728` | 5.03 |
| 14 | `cocoa` | 可可 | `#9a6b4f` | `#866652` | 5.00 |
| 15 | `lime` | 青柠 | `#65a30d` | `#50771A` | 5.05 |
| 16 | `steel` | 钢蓝 | `#4b7ea8` | `#4D708C` | 5.03 |
| 17 | `indigo` | 靛夜 | `#8b9cff` | `#99A6F0` | 7.80 |
| 18 | `plum` | 梅夜 | `#cf8ad0` | `#C693C7` | 7.21 |
| 19 | `pine` | 松夜 | `#6fc39a` | `#7AB99A` | 7.89 |
| 20 | `slate` | 岩夜 | `#93b6cc` | `#9AB4C5` | 8.36 |
| 21 | `aurora` | 极光青 | `#4fd1c5` | `#5FC1B8` | 8.39 |

**只改主色是不够的。** 每套主题的另 34 个键必须同时归位，否则会出现「按钮是新绿、边框还是旧蓝灰」的错配。所以迁移脚本不是「替换主色」，而是「用 `makeVars()` 重新生成全部 38 键」。

### 6.2 新增两套主题

```ts
import { paperColors, nightColors, toCssVars } from '@/design/tokens'

export const paper: ThemeDefinition = {
  id: 'paper',
  name: '宣纸',
  desc: '默认 · 暖纸墨青',
  dark: false,
  swatches: ['#F3F1EC', '#FBFAF7', '#0F6B64', '#14181B'],
  vars: toCssVars(paperColors),
}

export const night: ThemeDefinition = {
  id: 'night',
  name: '夜航舱',
  desc: '深色 · 长时间读数据',
  dark: true,
  swatches: ['#0E1215', '#12171B', '#56B9AC', '#E9E6E0'],
  vars: toCssVars(nightColors),
}
```

三处配套改动：

1. `ThemeId` 联合类型追加 `'paper' | 'night'`（其余 21 个**原样保留**）。
2. `themes` 数组把 `paper` **放在第一位**（设置页主题网格按数组顺序渲染，默认主题应在首位）。
3. `loadThemeId()` 的兜底返回值 `'ocean'` → `'paper'`。**只影响「从未保存过主题」的新用户**；老用户已存的 `astroman.theme` 原样命中，不会被改。

```ts
export function loadThemeId(): ThemeId {
  try {
    const stored =
      localStorage.getItem(STORAGE_KEY) ??
      LEGACY_STORAGE_KEYS.map((key) => localStorage.getItem(key)).find((value) => value)
    if (stored && themes.some((item) => item.id === stored)) return stored as ThemeId
  } catch {
    // localStorage 不可用时回退默认主题
  }
  return 'paper'   // ← 改这里
}
```

`ThemeDefinition` 可选扩展（不强制，加了便于设置页展示分组）：

```ts
export interface ThemeDefinition {
  id: ThemeId
  name: string
  desc: string
  dark: boolean
  swatches: string[]
  vars: Record<string, string>
  meta?: {
    family: 'paper' | 'ink' | 'night'
    contrast: number   // 该套最差对比度，写进代码便于复核
  }
}
```

### 6.3 迁移脚本 `scripts/theme-migrate.mjs`（完整源码）

一次性脚本。读旧 `theme.ts`，解析 21 套，重新生成 38 键，输出 `theme.generated.ts`。

```js
#!/usr/bin/env node
/**
 * 21 套主题重映射（一次性脚本）
 *
 * 用法：
 *   node scripts/theme-migrate.mjs            # 生成 theme.generated.ts 并打印对比度表
 *   node scripts/theme-migrate.mjs --check    # 只自检，不写文件（CI 用）
 *
 * 规则：
 *   1. 主色按 PRIMARY_MAP 固定映射（保色相，降饱和，明度达标）
 *   2. 其余 37 键按亮/暗分组套用 tokens.ts 的基准组
 *   3. 每套主题自检最差对比度，< 4.5 退出码 1
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const SRC = path.join(ROOT, 'src/stores/theme.ts')
const OUT = path.join(ROOT, 'src/stores/theme.generated.ts')
const CHECK_ONLY = process.argv.includes('--check')

/* ---------- 主色映射表：来自设计稿实算，不要改成运行时随机 ---------- */
const PRIMARY_MAP = {
  ocean: '#445FE3', graphite: '#4F6A86', forest: '#327959', midnight: '#90ABE2',
  amber: '#9D5C29', teal: '#1B7870', rose: '#BF3A68', clay: '#9E5950',
  dusk: '#A3B4F2', violet: '#7155D8', sky: '#1F749A', mint: '#1C7A5B',
  coral: '#B94728', cocoa: '#866652', lime: '#50771A', steel: '#4D708C',
  indigo: '#99A6F0', plum: '#C693C7', pine: '#7AB99A', slate: '#9AB4C5',
  aurora: '#5FC1B8',
}

/* ---------- 基准组：与 tokens.ts 的 paperColors / nightColors 必须一致 ---------- */
const PAPER = {
  'primary-soft': '#E4EDEA', 'success': '#2A7253', 'success-soft': '#E7F0EB',
  'warning': '#8F6118', 'warning-soft': '#F6EEE0',
  'danger': '#A83A2C', 'danger-soft': '#F6E7E4',
  'purple': '#6A4A87', 'purple-soft': '#EFEAF4',
  'bg': '#F3F1EC', 'surface': '#FBFAF7', 'surface-alt': '#F0EEE8',
  'border': '#DBD6CB', 'border-strong': '#C9C2B5', 'border-control': '#C9C2B5',
  'text': '#14181B', 'text-2': '#39424A', 'muted': '#616C74',
  'sidebar': '#F7F5F0', 'sidebar-hover': '#EFEDE6', 'sidebar-active': '#E4EDEA',
  'sidebar-text': '#4E585F', 'sidebar-icon': '#6C777E',
  'code-bg': '#FBFAF7', 'gutter-bg': '#F0EEE8',
  'json-key': '#2B5D8C', 'json-string': '#2A7253', 'json-number': '#8F6118',
  'json-bool': '#6A4A87', 'json-punct': '#8B959B',
  'shadow-sm': 'none', 'shadow': 'none',
  'shadow-lg': '0 18px 48px rgba(20, 24, 27, 0.16)',
  'brass': '#A97C3F', 'brass-text': '#8A6530',
}

const NIGHT = {
  'primary-soft': '#16302E', 'success': '#7FB79A', 'success-soft': '#16241D',
  'warning': '#C9A063', 'warning-soft': '#2A2317',
  'danger': '#D0705F', 'danger-soft': '#2E1C19',
  'purple': '#B49FD0', 'purple-soft': '#232030',
  'bg': '#0E1215', 'surface': '#12171B', 'surface-alt': '#171D22',
  'border': '#2C353D', 'border-strong': '#35404A', 'border-control': '#35404A',
  'text': '#E9E6E0', 'text-2': '#B9BFC3', 'muted': '#7E888E',
  'sidebar': '#11161A', 'sidebar-hover': '#1A2126', 'sidebar-active': '#1D272D',
  'sidebar-text': '#98A2A8', 'sidebar-icon': '#7E888E',
  'code-bg': '#10151A', 'gutter-bg': '#141A20',
  'json-key': '#7FA8C9', 'json-string': '#7FB79A', 'json-number': '#C9A063',
  'json-bool': '#B49FD0', 'json-punct': '#6F7A80',
  'shadow-sm': 'none', 'shadow': 'none',
  'shadow-lg': '0 18px 48px rgba(0, 0, 0, 0.5)',
  'brass': '#C9A063', 'brass-text': '#C9A063',
}

const ORDER = [
  'primary', 'primary-dark', 'primary-soft',
  'success', 'success-soft', 'warning', 'warning-soft', 'danger', 'danger-soft',
  'purple', 'purple-soft',
  'bg', 'surface', 'surface-alt', 'border', 'border-strong', 'border-control',
  'text', 'text-2', 'muted',
  'sidebar', 'sidebar-hover', 'sidebar-active', 'sidebar-text', 'sidebar-icon', 'sidebar-active-text',
  'code-bg', 'gutter-bg',
  'json-key', 'json-string', 'json-number', 'json-bool', 'json-punct',
  'shadow-sm', 'shadow', 'shadow-lg', 'brass', 'brass-text',
]

/* ---------- 色彩工具 ---------- */
const hex2rgb = (h) => {
  const s = h.replace('#', '')
  const f = s.length === 3 ? s.split('').map((c) => c + c).join('') : s
  return [parseInt(f.slice(0, 2), 16), parseInt(f.slice(2, 4), 16), parseInt(f.slice(4, 6), 16)]
}
const rgb2hex = (r, g, b) =>
  '#' + [r, g, b].map((n) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0')).join('').toUpperCase()
const mix = (a, b, t) => {
  const [r1, g1, b1] = hex2rgb(a); const [r2, g2, b2] = hex2rgb(b)
  return rgb2hex(r1 + (r2 - r1) * t, g1 + (g2 - g1) * t, b1 + (b2 - b1) * t)
}
const shade = (h, amt) => (amt >= 0 ? mix(h, '#FFFFFF', amt) : mix(h, '#000000', -amt))

const lum = (hex) => {
  const [r, g, b] = hex2rgb(hex).map((v) => v / 255)
  const f = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}
const contrast = (a, b) => {
  const la = lum(a); const lb = lum(b); const hi = Math.max(la, lb); const lo = Math.min(la, lb)
  return (hi + 0.05) / (lo + 0.05)
}

/* ---------- 解析旧 theme.ts ---------- */
const source = fs.readFileSync(SRC, 'utf8')
const blocks = []
const re = /const (\w+): ThemeDefinition = \{([\s\S]*?)\r?\n\}\r?\n/g
let m
while ((m = re.exec(source))) {
  const body = m[2]
  const pick = (k) => (body.match(new RegExp(`\\b${k}: '([^']*)'`)) || [])[1]
  const vars = {}
  const vre = /'(--fm-[\w-]+)':\s*'([^']*)'/g
  let v
  while ((v = vre.exec(body))) vars[v[1]] = v[2]
  blocks.push({
    varName: m[1],
    id: pick('id'),
    name: pick('name'),
    desc: pick('desc'),
    dark: /dark: true/.test(body),
    swatches: (body.match(/swatches: \[([^\]]+)\]/) || [])[1],
    oldPrimary: vars['--fm-primary'],
    oldVars: vars,
  })
}

if (blocks.length !== 21) {
  console.error(`✗ 解析到 ${blocks.length} 套主题，预期 21 套。请检查 theme.ts 结构是否被改动。`)
  process.exit(1)
}

/* ---------- 生成新 vars + 自检 ---------- */
const rows = []
const out = []

for (const b of blocks) {
  const primary = PRIMARY_MAP[b.id]
  if (!primary) {
    console.error(`✗ ${b.id} 不在 PRIMARY_MAP 中，请先补映射值`)
    process.exit(1)
  }
  const base = b.dark ? NIGHT : PAPER
  const vars = {
    ...base,
    'primary': primary,
    'primary-dark': b.dark ? shade(primary, 0.12) : shade(primary, -0.14),
    'primary-soft': b.dark ? mix(primary, '#000000', 0.82) : mix(primary, b.dark ? '#12171B' : '#FBFAF7', 0.88),
    'sidebar-active-text': primary,
  }

  const missing = ORDER.filter((k) => !vars[k])
  if (missing.length) {
    console.error(`✗ ${b.id} 生成后缺键: ${missing.join(', ')}`)
    process.exit(1)
  }

  const surface = vars['surface']
  const alt = vars['surface-alt']
  const checks = [
    ['text', vars['text'], surface],
    ['text-2', vars['text-2'], surface],
    ['muted', vars['muted'], alt],
    ['primary', vars['primary'], surface],
    ['success', vars['success'], alt],
    ['warning', vars['warning'], alt],
    ['danger', vars['danger'], alt],
  ]
  const worst = Math.min(...checks.map(([, f, bg]) => contrast(f, bg)))
  const primaryCr = contrast(primary, surface)

  rows.push({
    id: b.id, name: b.name, dark: b.dark,
    old: b.oldPrimary, next: primary,
    primaryCr: primaryCr.toFixed(2),
    worst: worst.toFixed(2),
    pass: worst >= 4.5 && primaryCr >= 4.5,
  })

  const varLines = ORDER.map((k) => `    '--fm-${k}': '${vars[k]}',`).join('\n')
  out.push(
`export const ${b.varName}: ThemeDefinition = {
  id: '${b.id}',
  name: '${b.name}',
  desc: '${b.desc}',
  dark: ${b.dark},
  swatches: [${b.swatches}],
  vars: {
${varLines}
  },
}`)
}

/* ---------- 打印对比度表 ---------- */
console.log('')
console.log('| # | 主题 | 旧主色 | 新主色 | 主色/底色 | 全套最差 | 判定 |')
console.log('|---|---|---|---|---|---|---|')
console.log('|---|---|---|---|---|---|---|')
rows.forEach((r, i) => {
  console.log(`| ${String(i + 1).padStart(2, '0')} | ${r.name} \`${r.id}\` | \`${r.old}\` | \`${r.next}\` | ${r.primaryCr} | ${r.worst} | ${r.pass ? '✅' : '❌'} |`)
})

const failed = rows.filter((r) => !r.pass)
if (failed.length) {
  console.error(`\n✗ ${failed.length} 套主题对比度未达 4.5：${failed.map((r) => r.id).join(', ')}`)
  process.exit(1)
}
console.log(`\n✓ ${rows.length} 套主题全部达标`)

if (CHECK_ONLY) process.exit(0)

/* ---------- 输出 ---------- */
const header = `/**
 * 本文件由 scripts/theme-migrate.mjs 生成，请勿手工编辑。
 * 生成时间：${new Date().toISOString()}
 * 来源：src/stores/theme.ts + src/design/tokens.ts
 */
import type { ThemeDefinition } from './theme'

`
fs.writeFileSync(OUT, header + out.join('\n\n') + '\n', 'utf8')
console.log(`✓ 已写入 ${path.relative(ROOT, OUT)}`)
```

**使用步骤**

```bash
# 1. 先跑自检，确认 21 套全部达标
node scripts/theme-migrate.mjs --check

# 2. 生成文件
node scripts/theme-migrate.mjs

# 3. 人工比对：把 theme.generated.ts 里的 21 个常量块
#    整体替换进 src/stores/theme.ts（保留文件尾部的 themes / applyTheme / loadThemeId 等函数）
git diff src/stores/theme.ts | head -120

# 4. 重命名保持导出名不变，删掉 theme.generated.ts
```

> **不要**把 `theme.generated.ts` 直接留在仓库里双份维护——它是过渡产物，替换完即删。保留一个 tag 即可回滚。

### 6.4 兼容性清单

| 项 | 处理 |
|---|---|
| `localStorage['astroman.theme']` | 不变，旧值 `ocean` / `midnight` 等照常命中 |
| `LEGACY_STORAGE_KEYS`（`fbinhttp.theme`、`finkmeter.theme`） | 保留，不动 |
| `document.documentElement.dataset.theme` | 保留（值仍是主题 id）；新增 `dataset.palette` |
| 21 个 `ThemeId` 字面量 | 全部保留，只追加 `paper` / `night` |
| 设置页主题网格 | 从 21 个变 23 个，网格列数自适应即可，不写死 `slice(0, 21)` |
| `--fm-method-*` 方法色 | 不进 `theme.vars`，跨主题恒定（现状即如此） |

**检查是否有人写死过主题数量**：

```bash
grep -rn "21\b" src/views/Settings.vue src/App.vue | grep -iv "font\|line-height\|padding\|margin\|width\|height\|z-index"
```

若有 `slice(0, 21)` 或 `themes.length === 21` 之类，改为动态。

---

## 07 组件层 `components.css`（C2 提交，完整文件）

新增 `src/styles/components.css`。**只允许出现 `var(--fm-*)`，不允许字面量色值与圆角。** 这一层是界面稿的直接落地，覆盖 20 个视图共用的骨架。

```css
/* ============================================================
   「深空钛」组件层
   只引用 L1/L1s 令牌；不写任何字面色值或字面圆角。
   ============================================================ */

/* ---------- 页面骨架 ---------- */
.fm-page {
  padding: 22px 24px 30px;
  background: var(--fm-bg);
  animation: fm-fade-in 160ms ease;
}

.fm-page-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--fm-gap-6);
  margin-bottom: 20px;
}

.fm-eyebrow {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: var(--fm-brass-text);
}

.fm-page-head h1 {
  margin: 8px 0 0;
  font-family: var(--fm-font-serif);
  font-size: 28px;
  font-weight: 600;
  line-height: 1.25;
  letter-spacing: 0.04em;
  color: var(--fm-text);
}

.fm-page-head .fm-sub {
  margin: 7px 0 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--fm-muted);
}

.fm-actions {
  display: flex;
  align-items: center;
  gap: var(--fm-gap-2);
  flex: none;
}

/* ---------- 面板：靠一条线分层，不靠投影 ---------- */
.fm-panel {
  background: var(--fm-surface);
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
}

.fm-panel > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fm-gap-3);
  padding: 12px 16px;
  border-bottom: 1px solid var(--fm-border);
}

.fm-panel > header h3 {
  margin: 0;
  font-size: 12.5px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--fm-text);
}

.fm-panel > header h3 span {
  margin-left: 8px;
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  font-weight: 400;
  letter-spacing: 0.14em;
  color: var(--fm-muted);
}

.fm-panel > header a {
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  color: var(--fm-primary);
  text-decoration: none;
  cursor: pointer;
}

.fm-panel-body { padding: 16px; }

/* 面板内元信息（右对齐的等宽小字） */
.fm-hstat {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.08em;
  color: var(--fm-muted);
}
.fm-hstat b { color: var(--fm-primary); font-weight: 600; font-size: 11px; }

/* ---------- 按钮 ---------- */
.fm-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--fm-border-control);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  color: var(--fm-text);
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  transition: border-color var(--fm-dur-fast) var(--fm-ease),
    background-color var(--fm-dur-fast) var(--fm-ease);
}
.fm-btn:hover {
  border-color: var(--fm-border-strong);
  background: var(--fm-surface-alt);
}
.fm-btn.primary {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
  color: var(--fm-surface);
}
.fm-btn.primary:hover {
  background: var(--fm-primary-dark);
  border-color: var(--fm-primary-dark);
}
.fm-btn.ghost {
  background: transparent;
  border-color: transparent;
  color: var(--fm-text-2);
}
.fm-btn.ghost:hover {
  background: var(--fm-surface-alt);
  border-color: var(--fm-border);
}
.fm-btn.danger {
  color: var(--fm-danger);
  border-color: color-mix(in srgb, var(--fm-danger) 35%, transparent);
  background: transparent;
}
.fm-btn.sm { height: 26px; padding: 0 9px; font-size: 11.5px; }
.fm-btn:disabled {
  color: var(--fm-muted);
  border-color: var(--fm-border);
  background: var(--fm-surface-alt);
  cursor: not-allowed;
}

/* ---------- 输入 ---------- */
.fm-input {
  width: 100%;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--fm-border-control);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  color: var(--fm-text);
  font-size: 12.5px;
  font-family: inherit;
  transition: border-color var(--fm-dur-fast) var(--fm-ease);
}
.fm-input::placeholder { color: var(--fm-muted); }
.fm-input:focus {
  outline: none;
  border-color: var(--fm-primary);
}
.fm-input.mono { font-family: var(--fm-font-mono); font-size: 11.5px; }

/* ---------- 侧栏：2px 关注线代替整块高亮 ---------- */
.fm-nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--fm-radius-sm);
  color: var(--fm-sidebar-text);
  font-size: 12.5px;
  cursor: pointer;
  transition: background-color var(--fm-dur-fast) var(--fm-ease),
    color var(--fm-dur-fast) var(--fm-ease);
}
.fm-nav-item::before {
  content: '';
  position: absolute;
  left: 0;
  top: 4px;
  bottom: 4px;
  width: 2px;
  background: transparent;
}
.fm-nav-item .fm-ico {
  display: inline-flex;
  color: var(--fm-sidebar-icon);
}
.fm-nav-item:hover { background: var(--fm-sidebar-hover); }
.fm-nav-item.active {
  background: var(--fm-sidebar-active);
  color: var(--fm-sidebar-active-text);
  font-weight: 600;
}
.fm-nav-item.active::before { background: var(--fm-primary); }
.fm-nav-item.active .fm-ico { color: var(--fm-sidebar-active-text); }

/* 分组标题 */
.fm-nav-group {
  padding: 14px 10px 6px;
  font-family: var(--fm-font-mono);
  font-size: 9px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--fm-muted);
}

/* ---------- 表格 ---------- */
.fm-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.fm-table th {
  padding: 8px 16px;
  border-bottom: 1px solid var(--fm-text);
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  font-weight: 400;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  text-align: left;
  color: var(--fm-muted);
  white-space: nowrap;
}
.fm-table td {
  padding: 9px 16px;
  border-bottom: 1px solid var(--fm-border);
  color: var(--fm-text-2);
  vertical-align: middle;
}
.fm-table tbody tr:last-child td { border-bottom: 0; }
.fm-table tbody tr:hover td { background: var(--fm-surface-alt); }
.fm-table .num {
  font-family: var(--fm-font-mono);
  font-variant-numeric: tabular-nums;
  text-align: right;
}
.fm-table .nm { color: var(--fm-text); font-weight: 600; }

/* ---------- 标签 / 方法徽标 ---------- */
.fm-pill {
  display: inline-flex;
  align-items: center;
  height: 18px;
  padding: 0 6px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius-sm);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.04em;
  color: var(--fm-muted);
  background: transparent;
}
.fm-pill.ok { color: var(--fm-success); border-color: color-mix(in srgb, var(--fm-success) 32%, transparent); }
.fm-pill.bad { color: var(--fm-danger); border-color: color-mix(in srgb, var(--fm-danger) 32%, transparent); }
.fm-pill.warn { color: var(--fm-warning); border-color: color-mix(in srgb, var(--fm-warning) 32%, transparent); }

.fm-mt {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  font-weight: 600;
  letter-spacing: 0.06em;
}
.fm-mt.get { color: var(--fm-method-get); }
.fm-mt.post { color: var(--fm-method-post); }
.fm-mt.put { color: var(--fm-method-put); }
.fm-mt.patch { color: var(--fm-method-patch); }
.fm-mt.delete { color: var(--fm-method-delete); }
.fm-mt.other { color: var(--fm-method-other); }

/* ---------- 代码块与行号 ---------- */
.fm-code {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
  padding: 12px 0;
  background: var(--fm-code-bg);
  border-radius: var(--fm-radius-sm);
  font-family: var(--fm-font-mono);
  font-size: 11.5px;
  line-height: 1.72;
  overflow: auto;
}
.fm-code .gutter {
  flex: none;
  padding: 0 10px 0 12px;
  text-align: right;
  color: var(--fm-muted);
  background: var(--fm-gutter-bg);
  border-right: 1px solid var(--fm-border);
  user-select: none;
  font-variant-numeric: tabular-nums;
}
.fm-code pre {
  margin: 0;
  padding: 0 14px;
  flex: 1;
  white-space: pre;
  color: var(--fm-text-2);
}
/* JSON 着色：只用手上的 5 个变量 */
.fm-code .k { color: var(--fm-json-key); }
.fm-code .s { color: var(--fm-json-string); }
.fm-code .n { color: var(--fm-json-number); }
.fm-code .b { color: var(--fm-json-bool); }
.fm-code .p { color: var(--fm-json-punct); }

/* ---------- 键值对（详情栏） ---------- */
.fm-kv {
  display: grid;
  grid-template-columns: 108px minmax(0, 1fr);
  gap: 4px 12px;
  padding: 10px 16px 14px;
  font-size: 11px;
}
.fm-kv .kk {
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-muted);
}
.fm-kv > span:not(.kk) {
  color: var(--fm-text-2);
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---------- 上手路径条（首屏唯一一处进度） ---------- */
.fm-pathband {
  display: flex;
  align-items: center;
  gap: 14px;
  height: 48px;
  padding: 0 16px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
}
.fm-pathband .label {
  display: flex;
  align-items: center;
  height: 20px;
  padding-right: 14px;
  border-right: 1px solid var(--fm-border);
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--fm-brass-text);
}
.fm-pathband .text {
  font-size: 12px;
  color: var(--fm-muted);
}
.fm-pathband .text b {
  color: var(--fm-text);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.fm-gauge {
  margin-left: auto;
  display: flex;
  align-items: flex-end;
  gap: 3px;
}
.fm-gauge i {
  width: 21px;
  height: 5px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
}
.fm-gauge i.on {
  background: var(--fm-primary);
  border-color: var(--fm-primary);
}
.fm-gauge i.cur {
  height: 9px;
  background: var(--fm-primary-soft);
  border-color: var(--fm-primary);
}
.fm-pathband .count {
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.08em;
  color: var(--fm-text-2);
}
.fm-pathband .link {
  font-family: var(--fm-font-mono);
  font-size: 10.5px;
  color: var(--fm-primary);
  cursor: pointer;
}

/* ---------- 手册步骤行 ---------- */
.fm-step {
  position: relative;
  display: flex;
  align-items: center;
  gap: 11px;
  min-height: 36px;
  padding: 0 14px 0 16px;
  border-bottom: 1px solid var(--fm-border);
  cursor: pointer;
}
.fm-step:last-child { border-bottom: 0; }
.fm-step::before {
  content: '';
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 2px;
  background: transparent;
}
.fm-step .no {
  flex: 0 0 15px;
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.06em;
  color: var(--fm-muted);
}
.fm-step .ti {
  flex: 0 0 100px;
  font-size: 12.5px;
  font-weight: 500;
  color: var(--fm-text);
  white-space: nowrap;
}
.fm-step .de {
  flex: 1;
  overflow: hidden;
  font-size: 11px;
  color: var(--fm-muted);
  white-space: nowrap;
  text-overflow: ellipsis;
}
.fm-step .go {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  color: var(--fm-primary);
  opacity: 0.45;
}
.fm-step.done::before { background: var(--fm-border); }
.fm-step.done .no { color: var(--fm-success); }
.fm-step.done .ti { font-weight: 400; color: var(--fm-text-2); }
.fm-step.now { background: var(--fm-primary-soft); }
.fm-step.now::before { background: var(--fm-primary); }
.fm-step.now .no { color: var(--fm-primary); font-weight: 600; }
.fm-step.now .go { opacity: 1; }

/* ---------- 问答 ---------- */
.fm-faq .q {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 36px;
  padding: 0 16px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 12.5px;
  color: var(--fm-text-2);
  cursor: pointer;
}
.fm-faq .q .sg {
  margin-left: auto;
  font-family: var(--fm-font-mono);
  font-size: 12px;
  color: var(--fm-brass-text);
}
.fm-faq .a {
  padding: 0 16px 11px;
  margin-top: -1px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11.5px;
  line-height: 1.75;
  color: var(--fm-muted);
}
.fm-faq .open { background: var(--fm-surface-alt); }
.fm-faq .open .q {
  border-bottom: 0;
  color: var(--fm-text);
  font-weight: 600;
  position: relative;
}
.fm-faq .open .q::before {
  content: '';
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--fm-brass);
}

/* ---------- 速查卡 ---------- */
.fm-tip {
  padding: 12px 14px 14px;
  border: 1px solid var(--fm-border);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
}
.fm-tip .th {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 7px;
}
.fm-tip .tn {
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.1em;
  color: var(--fm-brass-text);
}
.fm-tip .tt {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
}
.fm-tip .tg {
  margin-left: auto;
  padding: 1px 5px;
  border: 1px solid var(--fm-border);
  font-family: var(--fm-font-mono);
  font-size: 8.5px;
  letter-spacing: 0.08em;
  color: var(--fm-muted);
}
.fm-tip .td {
  min-height: 37px;
  margin: 0 0 9px;
  font-size: 11px;
  line-height: 1.7;
  color: var(--fm-muted);
}
.fm-tip pre {
  min-height: 69px;
  margin: 0;
  padding: 9px 10px;
  border: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  line-height: 1.7;
  color: var(--fm-text-2);
  white-space: pre-wrap;
  word-break: break-all;
}

/* ---------- 筛选 chip ---------- */
.fm-chips { display: flex; gap: 2px; margin-left: auto; }
.fm-chip {
  padding: 3px 9px;
  border: 1px solid transparent;
  font-family: var(--fm-font-mono);
  font-size: 11px;
  color: var(--fm-muted);
  white-space: nowrap;
  cursor: pointer;
}
.fm-chip:hover { color: var(--fm-text); }
.fm-chip.on {
  color: var(--fm-primary);
  border-color: var(--fm-border);
  background: var(--fm-surface);
}

/* ---------- 工具条 / 状态栏 ---------- */
.fm-toolbar {
  display: flex;
  align-items: center;
  gap: var(--fm-gap-2);
  padding: 8px 16px;
  border-bottom: 1px solid var(--fm-border);
  background: var(--fm-surface);
}
.fm-statusbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px;
  border-top: 1px solid var(--fm-border);
  background: var(--fm-surface);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-muted);
}
.fm-statusbar .d { color: var(--fm-border-strong); }
.fm-statusbar .right { margin-left: auto; }

/* ---------- KPI ---------- */
.fm-kpi {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 14px 16px;
  border: 1px solid var(--fm-border);
  border-right: 0;
  background: var(--fm-surface);
}
.fm-kpi:last-child { border-right: 1px solid var(--fm-border); }
.fm-kpi .lab {
  font-family: var(--fm-font-mono);
  font-size: 9px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--fm-muted);
}
.fm-kpi .val {
  font-family: var(--fm-font-mono);
  font-size: 22px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  color: var(--fm-text);
  line-height: 1.1;
}
.fm-kpi .val em {
  font-style: normal;
  font-size: 11px;
  color: var(--fm-muted);
  margin-left: 3px;
}
.fm-kpi .delta {
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-muted);
}
.fm-kpi .delta.up { color: var(--fm-success); }
.fm-kpi .delta.down { color: var(--fm-danger); }

/* ---------- 空状态 ---------- */
.fm-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 40px 20px;
  color: var(--fm-muted);
  font-size: 12px;
}
.fm-empty .mark {
  font-family: var(--fm-font-mono);
  font-size: 9px;
  letter-spacing: 0.2em;
  color: var(--fm-border-strong);
  text-transform: uppercase;
}

/* ---------- 焦点与可达性 ---------- */
.fm-btn:focus-visible,
.fm-input:focus-visible,
.fm-nav-item:focus-visible,
.fm-step:focus-visible,
.fm-chip:focus-visible,
.fm-faq .q:focus-visible {
  outline: 2px solid var(--fm-primary);
  outline-offset: 1px;
}

/* ---------- 降级：高对比偏好 ---------- */
@media (prefers-contrast: more) {
  .fm-panel,
  .fm-table td,
  .fm-tip { border-color: var(--fm-border-strong); }
  .fm-panel .fm-sub,
  .fm-tip .td,
  .fm-step .de { color: var(--fm-text-2); }
}
```

**接入方式**：`main.ts` 里 `import './styles/components.css'`。**不要**在每个视图的 `<style>` 里重写这些类——视图只负责套用 class，样式统一在这里改。

### 7.1 与设计稿的类名对照

界面稿用的是短类名（`.panel`、`.btn`、`.step`），代码里统一加 `fm-` 前缀，避免与 AntD、业务类名撞车。映射如下：

| 设计稿 | 代码 | 备注 |
|---|---|---|
| `.app` / `.rail` / `.work` | 保留现状（`App.vue` 已有） | 只改内部样式，不改结构类名 |
| `.page-head` / `.eyebrow` / `.sub` | `.fm-page-head` / `.fm-eyebrow` / `.fm-sub` | |
| `.panel` | `.fm-panel` | |
| `.btn` / `.ghost` | `.fm-btn` / `.fm-btn.ghost` | 与 AntD `a-button` 并存，自绘区域用 |
| `.tbl` | `.fm-table` | |
| `.step` / `.now` / `.done` | `.fm-step` / `.fm-step.now` / `.fm-step.done` | |
| `.tipx` | `.fm-tip` | |
| `.code` / `.gutter` | `.fm-code` / `.fm-code .gutter` | |
| `.kv` / `.kk` | `.fm-kv` / `.fm-kv .kk` | |
| `.pathband` / `.gauge` | `.fm-pathband` / `.fm-gauge` | |
| `.statusbar` | `.fm-statusbar` | |
| `.pill` / `.mt` | `.fm-pill` / `.fm-mt` | |

> 命名冲突提醒：界面稿里 `.sw` 同时被「色卡」和「开关」用过。代码中已分别落到 `.fm-swatch` 与 `.fm-tgl`，不要再复用短名。

---

## 08 视图改造（C3 提交）

### 8.1 通用清理规则（先批量，再手工）

**可以安全批量替换**（在 `src/views/*.vue` 的 `<style>` 块内）：

| 匹配 | 替换为 | 说明 |
|---|---|---|
| `border-radius: 6px` `8px` `14px` `10px` `12px` | `border-radius: var(--fm-radius)`（大面板/弹窗用 `--fm-radius-lg`） | **排除 `50%` 与 `999px`** |
| `box-shadow: 0 1px 2px …` `0 2px 6px …` | 整行删除 | 面板靠 1px 描边分层 |
| `box-shadow: 0 4px 14px …`（悬停态） | `box-shadow: none` + 加 `border-color: var(--fm-border-strong)` | 悬浮靠描边加深 |
| `background: #fff` `#ffffff` `#FFFFFF` | `background: var(--fm-surface)` | |
| `background: #fafbfc` `#f7f8fa` `#f5f6fa` | `background: var(--fm-bg)` | 页面底色 |
| `background: #f2f4f9` `#f4f6fb` | `background: var(--fm-surface-alt)` | 次级面板 |
| `border: 1px solid #e6e9f2` | `border: 1px solid var(--fm-border)` | |
| `color: #151b28` | `color: var(--fm-text)` | |
| `color: #6b7490` `#8b94ad` | `color: var(--fm-muted)` | |
| `rgba(0, 0, 0, 0.05)` | `color-mix(in srgb, var(--fm-text) 5%, transparent)` | 深色下自动反相 |
| `font-family: 'JetBrains Mono', Consolas, monospace` | `font-family: var(--fm-font-mono)` | |

**禁止自动替换**（会破坏图形）：

| 匹配 | 原因 |
|---|---|
| `border-radius: 50%` | 圆点、状态灯、头像 |
| `border-radius: 999px` | 胶囊标签，可保留（但新代码用 `--fm-radius-sm`） |
| `box-shadow: inset …` | 内阴影多用于分隔，删除会丢失结构 |
| SVG 内的 `fill="#..."` / `stroke="#..."` | 图表配色需单独处理，见 §8.4 |
| `filter: drop-shadow(...)` | 同投影问题，但需人工判断是否用于图标 |

**批量替换脚本会误伤，务必先跑一遍 diff 检查**：

```bash
cd /d/Software/ProjectFiles/AstroMan
# 只在 <style> 段内替换的粗暴做法：先备份
git checkout -b ui-redesign 2>/dev/null || git checkout ui-redesign
cp -r src/views /tmp/views-backup-$(date +%s)

# 示例：圆角（排除 50% 与 999px）
grep -rl "border-radius: \(6px\|8px\|10px\|12px\|14px\)" src/views \
  | xargs sed -i -E 's/border-radius: (6px|8px|10px|12px|14px);/border-radius: var(--fm-radius);/g'

git diff --stat src/views
```

> 注意：`sed -i` 在 Windows Git Bash 下可用，但会丢失文件权限位；Tauri 项目无影响。替换后**必须**逐文件 `git diff` 检查，特别是 `.vue` 文件里 `<style scoped>` 之外的模板内联 `style="..."`——模板内联样式也是硬编码来源，但 `sed` 只替换到 `border-radius:` 字面，模板里写 `style="border-radius: 8px"` 也会被命中，这类是可接受的。

### 8.2 20 个视图逐项改造清单

行数为实测。**「必改点」列出该文件里必须手工处理的非通用位置**，通用替换（§8.1）先跑完再做。

> ⚠️ **01 行（`Dashboard.vue`）的这条清单已被 2026-11-10「按初稿图 01 重做工作台」覆盖**：实际的落地结果是一个双栏 console 区（左侧标题/动作 + 右侧 2×2 KPI 组）、最近项目表 + 今日待办双栏、4 个入口一行、十格上手路径 + 底部状态条，而不是本条里的「KPI 行 / 快捷入口 4 卡 / `.fm-pathband`」三件事。行数也从 2,482 涨到约 2,990。**以后以 C3 标题下「C3-3 工作台落地记录」和文末偏差 7–11 为准**，本行仅作历史对照，不要在它上面继续改。

| # | 文件 | 行数 | 必改点 | 风险 |
|---|---|---|---|---|
| 01 | `Dashboard.vue` | 2,482 | ① 首屏 KPI 行改用 `.fm-kpi` 组；② 快捷入口 4 个卡片去掉投影与渐变底；③ 底部加 `.fm-pathband`；④ 教程区块（1300–1360 行）保留但折叠，见 §9.2 | 中 |
| 02 | `Projects.vue` | 303 | 项目卡片：`border-radius: 10px` → `3px`；卡片悬停从 `translateY(-2px)` 改为描边加深 | 低 |
| 03 | `Project.vue` | 1,603 | 左树：选中态用 2px 左线 + `--fm-primary-soft` 底；树节点高度 28px；接口列表行高 28px | 中 |
| 04 | `RequestEditor.vue` | 6,164 | 见 §8.3（专项） | **高** |
| 05 | `Environment.vue` | 815 | 变量表两列等宽对齐；密钥显隐按钮改 16px 图标；新增行输入框高度 30px | 低 |
| 06 | `StressTest.vue` | 2,215 | 参数表单两列栅格；运行态读数用 `.fm-kpi`；阶梯模式图改 SVG 描边风格 | 中 |
| 07 | `ReportDetail.vue` | 2,459 | 见 §8.4（专项） | **高** |
| 08 | `Reports.vue` | 25 | 包装页，无需单独改（跟随表格样式） | 低 |
| 09 | `Capture.vue` | 3,813 | 见 §8.5（专项） | **高** |
| 10 | `Runner.vue` | 1,066 | 三步向导：步骤指示改数字 + 2px 连线；结果表用 `.fm-table` | 中 |
| 11 | `Automation.vue` | 1,781 | 左套件右用例双栏：`grid-template-columns: 260px 1fr`；断言列表行高 28px | 中 |
| 12 | `History.vue` | 712 | 按时间分组：分组标题用 `.fm-nav-group` 样式（等宽小字） | 低 |
| 13 | `AiAssistant.vue` | 6,034 | 见 §8.6（专项） | **高** |
| 14 | `AiToolLogs.vue` | 955 | 日志流用 `.fm-code`；每条工具调用改成「时间 + 工具名 + 耗时」三列等宽 | 低 |
| 15 | `AiUsage.vue` | 1,242 | token 用量小图改 SVG 柱状（描边 + 主色填充 30%）；表格数字 `tabular-nums` | 低 |
| 16 | `Notes.vue` | 2,102 | 待办行 32px；完成态用删除线 + `--fm-muted`；右侧输入区贴底 | 中 |
| 17 | `QuickCapture.vue` | 301 | 240×140 无边框小窗；输入区铺满；去掉所有圆角与投影 | 低 |
| 18 | `Tools.vue` | 563 | 左工具列表 200px + 右面板；工具项选中态用 2px 左线 | 低 |
| 19 | `SystemMonitor.vue` | 1,639 | 读数卡 `.fm-kpi`；折线图改 `var(--fm-primary)` 描边 + 8% 面积填充 | 中 |
| 20 | `Settings.vue` | 2,723 | 左分组右表单；主题网格 23 套；主题卡去投影、选中态 2px 描边 | 中 |

### 8.3 `RequestEditor.vue` 专项（6,164 行，最高风险）

**只改 `<style>`，不动逻辑。** 三处关键：

**① URL 行改成一条命令行（32px）**

```css
/* 找到现有 .url-bar / .request-line 类（grep -n "url" src/views/RequestEditor.vue | head） */
.url-bar {
  display: flex;
  align-items: center;
  gap: 0;
  height: 32px;
  border: 1px solid var(--fm-border-control);
  border-radius: var(--fm-radius);
  background: var(--fm-surface);
  overflow: hidden;
}
.url-bar .method {
  flex: none;
  padding: 0 10px;
  border-right: 1px solid var(--fm-border);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  letter-spacing: 0.06em;
  background: var(--fm-surface-alt);
  height: 100%;
  display: flex;
  align-items: center;
}
.url-bar input {
  flex: 1;
  height: 100%;
  padding: 0 12px;
  border: 0;
  background: transparent;
  font-family: var(--fm-font-mono);
  font-size: 11.5px;
  color: var(--fm-text);
}
.url-bar input:focus { outline: none; }
.url-bar:focus-within { border-color: var(--fm-primary); }
```

**② 请求/响应面板高度：显式 `flex-basis`，不用百分比**

```css
.request-pane {
  display: flex;
  flex-direction: column;
  flex: 0 0 282px;      /* 固定请求区高度，避免响应区把它挤扁 */
  min-height: 0;
}
.response-pane {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;        /* 占用剩余空间 */
  min-height: 0;         /* 关键：不加这行，内部滚动会失效 */
  overflow: hidden;
}
```

**③ 行号与代码必须同 `line-height`**

```css
.code-area { display: flex; font-family: var(--fm-font-mono); font-size: 11.5px; line-height: 1.72; }
.code-area .gutter {
  flex: none;
  padding: 0 10px 0 12px;
  text-align: right;
  color: var(--fm-muted);
  background: var(--fm-gutter-bg);
  border-right: 1px solid var(--fm-border);
  user-select: none;
}
.code-area pre { margin: 0; padding: 0 14px; font: inherit; line-height: inherit; }
```

> 行号错位的唯一原因就是两边 `line-height` 或 `font-size` 不同。**不要**给行号单独设 `font-size`。

**④ 断言条贴底**

```css
.assert-bar {
  display: flex;
  align-items: center;
  gap: var(--fm-gap-4);
  padding: 9px 16px;
  border-top: 1px solid var(--fm-border);
  background: var(--fm-surface-alt);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-muted);
  flex: none;
}
.assert-bar .ok { color: var(--fm-success); }
.assert-bar .rt { margin-left: auto; }
```

**验证**：打开任一请求 → 响应 JSON 完整可见、底部断言条不被裁切、滚动条只在响应区内、行号与内容逐行对齐。

### 8.4 `ReportDetail.vue` 专项（2,459 行）

> **2026-11-11 变更**：按用户反馈，报告页不再固定深色，已移除嵌套深色 provider 与 `data-palette="night"`，改为跟随当前主题。以下深色方案保留为历史参考。

**深色局部作用域**（全局浅色时压测报告仍用深色）：

```vue
<a-config-provider :theme="nightProviderTheme">
  <div class="report-scope" data-palette="night">
    ...
  </div>
</a-config-provider>
```

```ts
import { nightColors, toCssVars } from '@/design/tokens'

const nightProviderTheme = computed(() => ({
  ...theme.value,
  algorithm: AntTheme.darkAlgorithm,
  token: { ...theme.value.token, ...toAntToken(toCssVars(nightColors)) },
}))
```

**更省事的做法**：不要复制一套 AntD token，直接复用全局 `theme` computed，只把局部容器的 CSS 变量重绑：

```css
.report-scope[data-palette='night'] {
  /* 局部重绑 38 个变量中的关键项；其余继承 */
  --fm-bg: #0E1215;
  --fm-surface: #12171B;
  --fm-surface-alt: #171D22;
  --fm-border: #2C353D;
  --fm-border-strong: #35404A;
  --fm-border-control: #35404A;
  --fm-text: #E9E6E0;
  --fm-text-2: #B9BFC3;
  --fm-muted: #7E888E;
  --fm-primary: #56B9AC;
  --fm-primary-dark: #6ECBBE;
  --fm-primary-soft: #16302E;
  --fm-shadow: none;
  --fm-shadow-lg: 0 18px 48px rgba(0, 0, 0, 0.5);

  background: var(--fm-bg);
  color: var(--fm-text);
}
```

**图表（手写 SVG）改读变量**：

```vue
<!-- 改前：写死色值 -->
<path d="..." stroke="#4d6bfe" fill="url(#grad)" />
<!-- 改后：读变量 -->
<path d="..." stroke="var(--fm-primary)" fill="none" stroke-width="1.6" />
<path d="..." fill="var(--fm-primary-soft)" opacity="0.5" />
```

网格线用 `stroke="var(--fm-border)"`，坐标轴文字用 `fill="var(--fm-muted)"`。
**SVG 里禁止出现 `#` 开头的色值**——深色下会失真。若要渐变，用 `stop-color="var(--fm-primary)"` + `stop-opacity`，不要写死两个 hex。

**SLA 表格**：结论先行（`PASS` / `FAIL` 徽标用 `.fm-pill.ok/.bad`），指标表用 `.fm-table`，数字列右对齐 + `tabular-nums`。

### 8.5 `Capture.vue` 专项（3,813 行）

三栏布局：

```css
.capture-layout {
  display: grid;
  grid-template-columns: 260px minmax(0, 1fr) 380px;
  height: 100%;
  min-height: 0;
}
.capture-layout > * { min-height: 0; overflow: auto; }
```

列表行固定 28px（为后续虚拟化留条件）：

```css
.cap-row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 28px;
  padding: 0 12px;
  border-bottom: 1px solid var(--fm-border);
  font-size: 11.5px;
  cursor: pointer;
}
.cap-row:hover { background: var(--fm-surface-alt); }
.cap-row.active { background: var(--fm-primary-soft); box-shadow: inset 2px 0 0 var(--fm-primary); }
.cap-row .path { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--fm-font-mono); }
```

详情栏内部：各区块 `flex: none`，**只让最后一个区块 `flex: 1`**。这是「详情栏内容溢出被裁」的通用解法：

```css
.detail-col { display: flex; flex-direction: column; min-height: 0; }
.detail-col > .block { flex: none; }
.detail-col > .block.grow { flex: 1 1 auto; min-height: 0; overflow: auto; }
```

### 8.6 `AiAssistant.vue` 专项（6,034 行）

**气泡不用背景色区分角色**，改左侧 2px 关注线 + 等宽标签：

```css
.msg { padding: 12px 16px 12px 18px; position: relative; border-bottom: 1px solid var(--fm-border); }
.msg::before { content: ''; position: absolute; left: 0; top: 12px; bottom: 12px; width: 2px; background: transparent; }
.msg.user::before { background: var(--fm-border-strong); }
.msg.assistant::before { background: var(--fm-primary); }
.msg .who {
  font-family: var(--fm-font-mono);
  font-size: 9px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--fm-muted);
  margin-bottom: 6px;
}
```

Markdown 渲染区复用 `.fm-code`：

```css
.msg :deep(pre) { background: var(--fm-surface-alt); border: 1px solid var(--fm-border); border-radius: var(--fm-radius-sm); padding: 10px 12px; font-size: 11.5px; line-height: 1.7; }
.msg :deep(code) { font-family: var(--fm-font-mono); font-size: 11px; background: var(--fm-surface-alt); padding: 1px 4px; }
.msg :deep(table) { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.msg :deep(th) { border-bottom: 1px solid var(--fm-text); text-align: left; font-weight: 500; padding: 6px 8px; }
.msg :deep(td) { border-bottom: 1px solid var(--fm-border); padding: 6px 8px; }
```

工具调用卡片：左侧 2px 青铜线 + 等宽工具名，与消息区分开：

```css
.tool-call { border: 1px solid var(--fm-border); border-left: 2px solid var(--fm-brass); border-radius: var(--fm-radius-sm); padding: 8px 10px; margin: 8px 0; font-family: var(--fm-font-mono); font-size: 10.5px; }
```

### 8.7 图标统一

现状约 262 处 `@ant-design/icons-vue` 引用，含 4 个 `Filled` 变体。**不改引入方式，只改尺寸与变体**：

```bash
# 找出 Filled 变体
grep -rn "CheckCircleFilled\|ExclamationCircleFilled\|PushpinFilled\|StarFilled" src --include=*.vue
```

替换为 `Outlined` 同名（`CheckCircleOutlined`、`ExclamationCircleOutlined`、`PushpinOutlined`、`StarOutlined`）。若某个 `Filled` 表达「已选中」语义（如 Star 收藏），保留语义但改用 `:style="{ color: 'var(--fm-primary)' }"`，不要用实心填充区分。

尺寸规则（在 `main.css` 加一条全局兜底，避免逐个视图改）：

```css
.anticon { font-size: 14px; }
.anticon svg { width: 1em; height: 1em; }
.fm-nav-item .anticon { font-size: 16px; }
```

颜色：`currentColor` 已被 AntD 默认继承，**不要**在图标上加 `color` 属性；由父级文字色控制。

---

## 09 使用说明页（C4 提交）

### 9.1 新增路由与导航

`src/router/index.ts`，在 `settings` 之前插入一条：

```ts
{ path: '/manual', name: 'manual', component: () => import('@/views/Manual.vue') },
```

**不要加 `meta.bare`**——加了会走裸布局，侧栏消失，页面无法返回。

`App.vue` 的 `navs`，在「系统」组、`settings` 之前插入：

```ts
{ key: 'manual', label: '使用说明', hint: '上手路径 · 常见问题 · 速查', to: '/manual', icon: ReadOutlined, group: '系统' },
```

`ReadOutlined` 需加入已有的图标 import：

```ts
import { ReadOutlined } from '@ant-design/icons-vue'
```

导航项从 12 → 13。**侧栏渲染逻辑不要写死分组数量或项数**，检查：

```bash
grep -n "navs.length\|slice(0, 4)\|group ===" src/App.vue | head
```

### 9.2 数据抽取：`src/data/manual.ts`

`Dashboard.vue` 里三组数据位置：`steps`（430 行，`computed`）、`tips`（503 行）、`faqs`（596 行）。

**三组数据的真实结构（已核对，决定了抽取方式）**：

| 数据 | 实际字段 | 能否原样搬运 |
|---|---|---|
| `steps` | `{ key, title, desc, action, go }`，且 `go` 是**函数**，引用了 `createOpen` / `goEnvironment` / `goProject` / `router`（模板用 `step.go()` 与 `step.action`） | **不能**。原样搬进 `manual.ts` 会带着这些标识符，tsc 立即报 `Cannot find name` |
| `tips` | `{ title, desc, code }`，纯字面量，**没有** `tag` / `group` | 文案可原样搬，分组要另给 |
| `faqs` | `{ q, a }`，纯字面量 | 可原样搬 |

所以抽取策略是**内容与动作分离**：

1. `manual.ts` 只存内容：`steps` 存 `key / title / desc / route`，把 `go` 函数换成路由字符串；`tips` 原样存文案，分组与序号单独放 `TIP_META`；`faqs` 原样。
2. `Dashboard.vue` 保留自己的 `stepActions` 映射（那 10 个动作依赖 `recentProject` / `createOpen`，本来就不属于数据层），用 `manualSteps` 拼回模板需要的 `action` / `go`。模板零改动。
3. `Manual.vue` 直接读 `route` 跳转，读 `TIP_META` 渲染序号与分组。

**抽取脚本 `scripts/extract-manual.mjs`**（一次性）：

```js
#!/usr/bin/env node
/**
 * 从 Dashboard.vue 抽取教程内容到 src/data/manual.ts
 *
 * 与旧版脚本的区别（重要）：
 *   steps 里的 go 是函数、action 是文案，不能进数据层 —— 这里只保留 key/title/desc，
 *   路由由 STEP_ROUTES 显式映射；动作留在 Dashboard.vue 的 stepActions 里。
 *   tips 原文只有 title/desc/code，tag/group 由 TIP_META 按下标补。
 *
 * 抽完必须跑 npm run build —— check-manual.mjs 只数条数，查不出类型错误。
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const SRC = path.join(ROOT, 'src/views/Dashboard.vue')
const OUT = path.join(ROOT, 'src/data/manual.ts')

const src = fs.readFileSync(SRC, 'utf8')

/* ---------- 步骤 → 路由（内容层只需要一个可点击的去处） ---------- */
const STEP_ROUTES = {
  '01': '/projects', '02': '/settings', '03': '/projects', '04': '/capture',
  '05': '/stress', '06': '/automation', '07': '/automation', '08': '/projects',
  '09': '/ai', '10': '/settings',
}

/* ---------- 速查卡分组：按下标与 manualTips 对齐，新增/删除 tip 必须同步改这里 ---------- */
const TIP_GROUPS = [
  '变量', '脚本', '脚本', '脚本', '脚本', '调试', '调试',
  '抓包', '抓包', '抓包', '压测', '自动化', '压测',
  '航仔', '航仔', '航仔', '航仔', '数据',
]

function sliceConst(startPattern) {
  const start = src.indexOf(startPattern)
  if (start < 0) throw new Error(`找不到 ${startPattern}`)
  // 从起始位置向后做括号配对，取到匹配的 ] 为止
  let i = src.indexOf('[', start)
  let depth = 0
  for (; i < src.length; i++) {
    if (src[i] === '[') depth++
    else if (src[i] === ']') {
      depth--
      if (depth === 0) return src.slice(src.indexOf('[', start), i + 1)
    }
  }
  throw new Error(`${startPattern} 括号不闭合`)
}

const stepsRaw = sliceConst('const steps = computed(() =>')
const tipsRaw = sliceConst('const tips = [')
const faqsRaw = sliceConst('const faqs = [')

/* ---------- steps：只取三个字段，函数与 action 丢掉 ---------- */
const steps = []
const stepRe = /key:\s*'([^']+)',\s*title:\s*'([^']*)',\s*desc:\s*'([^']*)'/g
let m
while ((m = stepRe.exec(stepsRaw))) {
  const route = STEP_ROUTES[m[1]]
  if (!route) {
    console.error(`✗ 步骤 ${m[1]} 不在 STEP_ROUTES 中，请先补路由`)
    process.exit(1)
  }
  steps.push({ key: m[1], title: m[2], desc: m[3], route })
}

/* ---------- tips / faqs：纯字面量，原样保留 ---------- */
const tipsCount = (tipsRaw.match(/^[ \t]*title:[ \t]*'/gm) || []).length
const faqsCount = (faqsRaw.match(/^[ \t]*q:[ \t]*'/gm) || []).length

if (steps.length !== 10 || tipsCount !== 18 || faqsCount !== 22) {
  console.error(`✗ 抽取条数异常：steps=${steps.length}(期望 10) tips=${tipsCount}(期望 18) faqs=${faqsCount}(期望 22)`)
  process.exit(1)
}
if (TIP_GROUPS.length !== tipsCount) {
  console.error(`✗ TIP_GROUPS 有 ${TIP_GROUPS.length} 项，与 tips ${tipsCount} 条不一致`)
  process.exit(1)
}

// 每条步骤输出为多行：check-manual.mjs 按行首 key: 计数，压成单行会数出 0
const stepsBody = steps
  .map(
    (s) =>
      `  {\n    key: '${s.key}',\n    title: '${s.title}',\n    desc: '${s.desc}',\n    route: '${s.route}',\n  },`,
  )
  .join('\n')

const metaBody = TIP_GROUPS
  .map((g, i) => `  { tag: 'T-${String(i + 1).padStart(2, '0')}', group: '${g}' },`)
  .join('\n')

const body = `/**
 * 使用说明数据源（从 Dashboard.vue 抽取，Dashboard 与 Manual 共用）
 * 条数基线：steps 10 / tips 18 / faqs 22 —— 由 scripts/check-manual.mjs 守护
 *
 * 注意：本文件只放“文案与去处”，不放函数。Dashboard 的步骤动作用 stepActions 另存。
 */

export interface ManualStep {
  key: string
  title: string
  desc: string
  /** Manual 页点击后跳转的路由；Dashboard 用自己更精确的动作覆盖 */
  route: string
}

export interface ManualTip {
  title: string
  desc: string
  code: string
}

export interface ManualFaq {
  q: string
  a: string
}

export const manualSteps: ManualStep[] = [
${stepsBody}
]

export const manualTips: ManualTip[] = ${tipsRaw}

export const manualFaqs: ManualFaq[] = ${faqsRaw}

/** 速查卡的序号与分组，下标与 manualTips 一一对应 */
export const TIP_META: { tag: string; group: string }[] = [
${metaBody}
]
`

fs.mkdirSync(path.dirname(OUT), { recursive: true })
fs.writeFileSync(OUT, body, 'utf8')
console.log(`✓ 已写入 ${path.relative(ROOT, OUT)}（steps ${steps.length} / tips ${tipsCount} / faqs ${faqsCount}）`)
```

**抽取后的 Dashboard 改法**（不动模板，只换数据来源）：

```ts
// 删除原 steps / tips / faqs 三个定义
import { manualSteps, manualTips, manualFaqs } from '@/data/manual'

// 动作留在 Dashboard：它们依赖 createOpen / recentProject / router，不属于数据层
const stepActions: Record<string, { action: string; go: () => void }> = {
  '01': { action: '新建项目', go: () => (createOpen.value = true) },
  '02': { action: '管理环境', go: goEnvironment },
  '03': { action: '打开项目', go: goProject },
  '04': { action: '打开抓包', go: () => router.push('/capture') },
  '05': { action: '去压测', go: () => router.push('/stress') },
  '06': { action: '打开自动化测试', go: () => router.push('/automation') },
  '07': { action: '打开项目', go: goProject },
  '08': { action: '打开项目', go: goProject },
  '09': { action: '打开航仔', go: () => router.push({ name: 'ai' }) },
  '10': { action: '打开设置', go: () => router.push('/settings') },
}

const steps = computed(() =>
  manualSteps.map((s) => {
    const a = stepActions[s.key]
    return { ...s, action: a?.action ?? '前往', go: a?.go ?? (() => router.push(s.route)) }
  }),
)
const tips = manualTips
const faqs = manualFaqs
```

> 模板里 `v-for="step in steps"`、`step.go()`、`step.action`、`tips`、`faqs` 全部**保持原样**——内容不变，动作由 `stepActions` 拼回，所以模板零改动。
>
> 不要写 `progress.value.done`：Dashboard 当前**没有**任何进度状态，那是 Manual 页新增的概念（见 §9.4），在 Dashboard 里引用会直接报未定义。路径条的进度由 `loadManualProgress()` 单独提供。

### 9.3 `src/views/Manual.vue`（完整文件）

```vue
<template>
  <div class="fm-page manual">
    <div class="fm-page-head">
      <div>
        <div class="fm-eyebrow">Manual / 001</div>
        <h1>使用说明</h1>
        <p class="fm-sub">
          十节走完就能上手；常见问题与常用操作速查同页可查，不必翻文档。
        </p>
      </div>
      <div class="fm-actions">
        <button class="fm-btn ghost" @click="scrollToSteps">回到步骤</button>
        <button class="fm-btn" @click="printManual">导出 PDF</button>
      </div>
    </div>

    <div class="man-grid">
      <!-- 左：上手路径 -->
      <section class="fm-panel">
        <header>
          <h3>上手路径<span>01 — 10</span></h3>
          <span class="fm-hstat">已完成 <b>{{ progress.done.length }}</b> / 10</span>
        </header>
        <div
          v-for="step in steps"
          :key="step.key"
          :class="['fm-step', stepState(step.key)]"
          tabindex="0"
          role="button"
          @click="openStep(step.key)"
          @keydown.enter="openStep(step.key)"
        >
          <span class="no">{{ step.key }}</span>
          <span class="ti">{{ step.title }}</span>
          <span class="de">{{ step.desc }}</span>
          <span class="go">{{ step.key === progress.current ? '继续 →' : '前往 →' }}</span>
        </div>
      </section>

      <!-- 右：常见问题 -->
      <section class="fm-panel">
        <header>
          <h3>常见问题<span>FAQ · {{ manualFaqs.length }}</span></h3>
          <a @click="focusSearch">搜索全部 →</a>
        </header>
        <div class="fm-faq">
          <div v-for="(item, i) in manualFaqs" :key="i" :class="{ open: openFaq === i }">
            <div
              class="q"
              tabindex="0"
              role="button"
              :aria-expanded="openFaq === i"
              @click="openFaq = openFaq === i ? -1 : i"
              @keydown.enter="openFaq = openFaq === i ? -1 : i"
            >
              {{ item.q }}<span class="sg">{{ openFaq === i ? '−' : '+' }}</span>
            </div>
            <div v-if="openFaq === i" class="a">{{ item.a }}</div>
          </div>
        </div>
        <div class="faq-foot">
          还有 <b>{{ manualFaqs.length - 1 }}</b> 条未展开 · 共 {{ manualFaqs.length }} 问
          <span>搜索全部问答 →</span>
        </div>
      </section>
    </div>

    <!-- 速查 -->
    <div class="cheat-head">
      <h3>常用操作速查<span>CHEAT SHEET</span></h3>
      <div class="fm-chips">
        <span
          v-for="g in groups"
          :key="g"
          :class="['fm-chip', { on: activeGroup === g }]"
          role="button"
          tabindex="0"
          @click="activeGroup = g"
          @keydown.enter="activeGroup = g"
        >{{ g }} {{ groupCount(g) }}</span>
      </div>
    </div>

    <div class="cheat-grid">
      <div v-for="tip in visibleTips" :key="tip.title" class="fm-tip">
        <div class="th">
          <span class="tn">{{ tip.tag }}</span>
          <span class="tt">{{ tip.title }}</span>
          <span class="tg">{{ tip.group }}</span>
        </div>
        <p class="td">{{ tip.desc }}</p>
        <pre>{{ tip.code }}</pre>
      </div>
    </div>

    <div class="fm-statusbar">
      <span>手册 v{{ version }}</span><span class="d">│</span>
      <span>10 章 · {{ manualTips.length }} 条速查 · {{ manualFaqs.length }} 问</span><span class="d">│</span>
      <span>离线可查</span>
      <span class="right">按 Ctrl F 搜索本章</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { version as packageVersion } from '../../package.json'
import { manualSteps, manualTips, manualFaqs, TIP_META, type ManualStep } from '@/data/manual'
import { loadManualProgress, saveManualProgress, type ManualProgress } from '@/composables/useManualProgress'

const router = useRouter()

// 版本从 package.json 读：项目有 version:check，写死会和发版脱节
const version = packageVersion

const steps = computed<ManualStep[]>(() => manualSteps)
const progress = ref<ManualProgress>({ done: [], current: '01' })
const openFaq = ref(0)
const activeGroup = ref('全部')

const GROUPS = ['变量', '脚本', '调试', '抓包', '压测', '自动化', '航仔', '数据']
const groups = computed(() => ['全部', ...GROUPS])

// 文案来自 manualTips，序号与分组来自 TIP_META（同序下标对齐）
const tips = computed(() =>
  manualTips.map((t, i) => ({
    ...t,
    tag: TIP_META[i]?.tag ?? `T-${String(i + 1).padStart(2, '0')}`,
    group: TIP_META[i]?.group ?? '其他',
  })),
)

function groupCount(g: string) {
  return g === '全部' ? tips.value.length : tips.value.filter((t) => t.group === g).length
}

const visibleTips = computed(() =>
  activeGroup.value === '全部'
    ? tips.value
    : tips.value.filter((t) => t.group === activeGroup.value),
)

function stepState(key: string) {
  if (progress.value.done.includes(key)) return 'done'
  if (progress.value.current === key) return 'now'
  return ''
}

function openStep(key: string) {
  const step = manualSteps.find((s) => s.key === key)
  if (!step) return
  progress.value.current = key
  saveManualProgress(progress.value)
  // 路由来自数据层（manual.ts 的 route），走 vue-router，不用 location.hash
  router.push(step.route)
}

function scrollToSteps() {
  document.querySelector('.man-grid')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

function focusSearch() {
  // 走浏览器原生查找，不做自研搜索框
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true }))
}

function printManual() {
  window.print()
}

onMounted(() => {
  progress.value = loadManualProgress()
  const cur = manualSteps.find((s) => s.key === progress.value.current)
  if (!cur && manualSteps[0]) progress.value.current = manualSteps[0].key
})
</script>

<style scoped>
.man-grid {
  display: grid;
  grid-template-columns: minmax(0, 470px) minmax(0, 1fr);
  gap: var(--fm-gap-4);
}
.man-grid .fm-panel {
  display: flex;
  flex-direction: column;
}
.man-grid .fm-step { flex: 1; }

.faq-foot {
  display: flex;
  align-items: center;
  margin-top: auto;
  padding: 10px 16px;
  border-top: 1px solid var(--fm-border);
  font-family: var(--fm-font-mono);
  font-size: 10px;
  color: var(--fm-muted);
}
.faq-foot b { color: var(--fm-text-2); font-weight: 500; }
.faq-foot span { margin-left: auto; color: var(--fm-primary); cursor: pointer; }

.cheat-head {
  display: flex;
  align-items: center;
  gap: var(--fm-gap-4);
  margin: 12px 0 9px;
}
.cheat-head h3 {
  margin: 0;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fm-text);
  letter-spacing: 0.02em;
}
.cheat-head h3 span {
  margin-left: 8px;
  font-family: var(--fm-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.14em;
  color: var(--fm-muted);
}
.cheat-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
}
.fm-statusbar { margin-top: var(--fm-gap-4); }

@media (max-width: 1180px) {
  .cheat-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
@media (max-width: 960px) {
  .man-grid { grid-template-columns: minmax(0, 1fr); }
  .cheat-grid { grid-template-columns: minmax(0, 1fr); }
}

@media print {
  .fm-page-head .fm-actions,
  .fm-statusbar { display: none; }
  .man-grid { grid-template-columns: 1fr 1fr; }
  .fm-panel { break-inside: avoid; }
}
</style>
```

### 9.4 `src/composables/useManualProgress.ts`（完整文件）

```ts
import type { Ref } from 'vue'

const KEY = 'astroman.manual.progress'

export interface ManualProgress {
  done: string[]
  current: string
  updatedAt: number
}

const EMPTY: ManualProgress = { done: [], current: '01', updatedAt: 0 }

export function loadManualProgress(): ManualProgress {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return { ...EMPTY }
    const parsed = JSON.parse(raw) as Partial<ManualProgress>
    return {
      done: Array.isArray(parsed.done) ? parsed.done.filter((k) => typeof k === 'string') : [],
      current: typeof parsed.current === 'string' ? parsed.current : '01',
      updatedAt: typeof parsed.updatedAt === 'number' ? parsed.updatedAt : 0,
    }
  } catch {
    return { ...EMPTY }
  }
}

export function saveManualProgress(p: ManualProgress) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...p, updatedAt: Date.now() }))
  } catch {
    // 存储不可用时静默失败，不影响页面可用性
  }
}

export function markStepDone(p: ManualProgress, key: string): ManualProgress {
  const done = p.done.includes(key) ? p.done : [...p.done, key]
  return { ...p, done, current: key }
}

export type { Ref }
```

> 进度存 `localStorage` 而不是 SQLite：它属于轻偏好，与 `astroman.theme`、`fm-zoom`、`astroman:sidebar-collapsed` 同级。写数据库要走 IPC 与迁移，收益不成比例。后续真要做跨设备同步再迁。

### 9.5 `src/components/manual/PathBand.vue`（完整文件）

```vue
<template>
  <div class="fm-pathband">
    <span class="label">上手路径</span>
    <span class="text">
      {{ total }} 节 · 已完成 <b>{{ pad(progress.done.length) }}</b> 节 · 下一步「{{ nextTitle }}」
    </span>
    <span class="fm-gauge">
      <i
        v-for="i in total"
        :key="i"
        :class="tickClass(i)"
        :title="`第 ${i} 节`"
        @click="go(i)"
      />
    </span>
    <span class="count">{{ pad(progress.done.length) }} / {{ pad(total) }}</span>
    <span class="link" @click="openManual">打开使用说明 →</span>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import type { ManualProgress } from '@/composables/useManualProgress'
import type { ManualStep } from '@/data/manual'

const props = defineProps<{
  progress: ManualProgress
  steps: ManualStep[]
}>()

const router = useRouter()
const total = computed(() => props.steps.length)

const pad = (n: number) => String(n).padStart(2, '0')

const nextTitle = computed(() => {
  const next = props.steps.find((s) => !props.progress.done.includes(s.key))
  return next?.title ?? '全部完成'
})

function tickClass(i: number) {
  const key = props.steps[i - 1]?.key
  if (!key) return ''
  if (props.progress.done.includes(key)) return 'on'
  if (props.progress.current === key) return 'cur'
  return ''
}

function go(i: number) {
  const key = props.steps[i - 1]?.key
  if (!key) return
  router.push('/manual')
}

function openManual() {
  router.push('/manual')
}
</script>
```

**接入 Dashboard**：在首屏最后一个区块之后插入

```vue
<PathBand :progress="manualProgress" :steps="manualSteps" />
```

```ts
import PathBand from '@/components/manual/PathBand.vue'
import { manualSteps } from '@/data/manual'
import { loadManualProgress } from '@/composables/useManualProgress'

const manualProgress = ref(loadManualProgress())
```

**必须在模板里放在首屏可见区域内**（KPI + 待办 + 快捷入口之后）。48px 单行，不打断视线。上线前用截图确认它没有被窗口底部裁掉。

---

## 10 质量门禁脚本（C0 一并提交）

### 10.1 `scripts/check-hardcoded-color.mjs`

```js
#!/usr/bin/env node
/**
 * 硬编码样式门禁：统计 hex / rgba / 字面圆角 / 投影 / :deep(.ant-*)
 * 超出基线即失败，防止改造期间「边改边加」。
 *
 * 用法：node scripts/check-hardcoded-color.mjs [--write-baseline]
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const TARGET_DIRS = ['src/views', 'src/components', 'src/App.vue']
const SRC_DIR = 'src'

/**
 * 基线（2026-11-10 C3 收尾实测，统计范围含 src/views + src/components + src/App.vue + src/styles）
 * hex 234 / rgba 19 / radius 447 / shadow 51 / deep 110
 * C3 结束后下调到实测值，此后只允许更低。
 *
 * 口径要点：src/styles/tokens.css 是令牌定义处（自带 59 个 hex），必须排除。
 * 否则 C0 一新增这个文件，hex 就变成 484，门禁当场自锁。
 */
const BASELINE = {
  hex: 234,
  rgba: 19,
  radius: 447,
  shadow: 51,
  deep: 110,
}

const EXT = new Set(['.vue', '.ts', '.css'])

function walk(dir, out = []) {
  if (!fs.existsSync(dir)) return out
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name)
    const st = fs.statSync(full)
    if (st.isDirectory()) {
      if (name === 'node_modules' || name.startsWith('.')) continue
      walk(full, out)
    } else if (EXT.has(path.extname(name))) {
      out.push(full)
    }
  }
  return out
}

const files = []
TARGET_DIRS.forEach((d) => {
  const full = path.join(ROOT, d)
  if (fs.statSync(full, { throwIfNoEntry: false })?.isDirectory()) walk(full, files)
  else if (fs.existsSync(full)) files.push(full)
})
// 样式目录单独统计
walk(path.join(ROOT, 'src/styles'), files)

const counts = { hex: 0, rgba: 0, radius: 0, shadow: 0, deep: 0 }
const perFile = {}

/* 令牌定义处不计入：tokens.css 天然是色值集中地 */
const TOKEN_FILES = new Set([path.join(ROOT, 'src/styles/tokens.css')])

for (const file of files) {
  if (TOKEN_FILES.has(file)) continue
  const text = fs.readFileSync(file, 'utf8')
  const c = {
    hex: (text.match(/#[0-9a-fA-F]{3,8}\b/g) || []).length,
    rgba: (text.match(/rgba?\(/g) || []).length,
    radius: (text.match(/border-radius\s*:/g) || []).length,
    shadow: (text.match(/box-shadow\s*:/g) || []).length,
    deep: (text.match(/:deep\(\s*\.ant-/g) || []).length,
  }
  Object.keys(c).forEach((k) => (counts[k] += c[k]))
  if (c.hex + c.rgba + c.radius + c.shadow + c.deep > 0) {
    perFile[path.relative(ROOT, file)] = c
  }
}

const rows = Object.entries(perFile)
  .sort((a, b) => b[1].hex + b[1].radius - (a[1].hex + a[1].radius))
  .slice(0, 15)

console.log('\n文件                              hex  rgba  radius  shadow  deep')
console.log('─'.repeat(70))
rows.forEach(([f, c]) => {
  console.log(
    `${f.padEnd(32)} ${String(c.hex).padStart(4)} ${String(c.rgba).padStart(5)} ${String(c.radius).padStart(7)} ${String(c.shadow).padStart(7)} ${String(c.deep).padStart(5)}`,
  )
})

console.log('\n合计：')
let failed = false
Object.entries(BASELINE).forEach(([k, base]) => {
  const now = counts[k]
  const ok = now <= base
  if (!ok) failed = true
  console.log(`  ${k.padEnd(8)} ${String(now).padStart(4)} / 基线 ${base}  ${ok ? '✅' : '❌ 超出'}`)
})

if (failed) {
  console.error('\n✗ 硬编码数量超过基线。本次改造只允许下降。')
  process.exit(1)
}
console.log('\n✓ 硬编码数量在基线内')
```

**基线需要随改造推进而下调**：C3 结束时把 `BASELINE` 改成实测值，再往后只允许更低。

### 10.2 `scripts/check-contrast.mjs`

```js
#!/usr/bin/env node
/**
 * 对比度门禁：读取 tokens.ts 的 paperColors / nightColors，
 * 逐对计算，任何一对 < 4.5（正文）或 < 3.0（大字）即失败。
 *
 * 用法：node scripts/check-contrast.mjs
 */
import fs from 'node:fs'
import path from 'node:path'

const SRC = path.join(process.cwd(), 'src/design/tokens.ts')
if (!fs.existsSync(SRC)) {
  console.error('✗ 未找到 src/design/tokens.ts')
  process.exit(1)
}

const src = fs.readFileSync(SRC, 'utf8')

function extractBlock(name) {
  const start = src.indexOf(`export const ${name}: ColorVars = {`)
  if (start < 0) throw new Error(`找不到 ${name}`)
  const open = src.indexOf('{', start)
  let depth = 0
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth++
    else if (src[i] === '}') {
      depth--
      if (depth === 0) return src.slice(open + 1, i)
    }
  }
  throw new Error(`${name} 不闭合`)
}

function parseColors(block) {
  const out = {}
  const re = /'([\w-]+)':\s*(?:primitive\.(\w+)|'(#[0-9a-fA-F]{3,8})')/g
  const primSrc = src.slice(src.indexOf('export const primitive'), src.indexOf('} as const', src.indexOf('export const primitive')))
  const prim = {}
  const pre = /(\w+):\s*'(#[0-9a-fA-F]{3,8})'/g
  let p
  while ((p = pre.exec(primSrc))) prim[p[1]] = p[2]

  let m
  while ((m = re.exec(block))) {
    out[m[1]] = m[2] ? prim[m[2]] : m[3]
  }
  return out
}

const hex2rgb = (h) => {
  const s = h.replace('#', '')
  const f = s.length === 3 ? s.split('').map((c) => c + c).join('') : s
  return [parseInt(f.slice(0, 2), 16), parseInt(f.slice(2, 4), 16), parseInt(f.slice(4, 6), 16)]
}
const lum = (hex) => {
  const [r, g, b] = hex2rgb(hex).map((v) => v / 255)
  const f = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}
const contrast = (a, b) => {
  const la = lum(a); const lb = lum(b); const hi = Math.max(la, lb); const lo = Math.min(la, lb)
  return (hi + 0.05) / (lo + 0.05)
}

function audit(label, colors) {
  const pairs = [
    ['text / surface', colors.text, colors.surface, 4.5],
    ['text-2 / surface', colors['text-2'], colors.surface, 4.5],
    ['muted / surface-alt', colors.muted, colors['surface-alt'], 4.5],
    ['primary / surface', colors.primary, colors.surface, 4.5],
    ['primary / bg', colors.primary, colors.bg, 4.5],
    ['surface / primary（主按钮文字）', colors.surface, colors.primary, 4.5],
    ['danger / surface-alt', colors.danger, colors['surface-alt'], 4.5],
    ['success / surface-alt', colors.success, colors['surface-alt'], 4.5],
    ['warning / surface-alt', colors.warning, colors['surface-alt'], 4.5],
    ['brass-text / bg', colors['brass-text'], colors.bg, 4.5],
    ['border-control / surface（非文字，≥1.5）', colors['border-control'], colors.surface, 1.5],
    ['border / surface（非文字，≥1.3）', colors.border, colors.surface, 1.3],
  ]

  console.log(`\n== ${label} ==`)
  let bad = 0
  pairs.forEach(([name, fg, bg, min]) => {
    if (!fg || !bg) {
      console.log(`  ⚠ ${name}：缺色值`)
      return
    }
    const c = contrast(fg, bg)
    const ok = c >= min
    if (!ok) bad++
    console.log(`  ${ok ? '✅' : '❌'} ${name} = ${c.toFixed(2)}（要求 ≥${min}）`)
  })
  return bad
}

const bad =
  audit('宣纸 paper', parseColors(extractBlock('paperColors'))) +
  audit('夜航舱 night', parseColors(extractBlock('nightColors')))

if (bad) {
  console.error(`\n✗ ${bad} 对未达标`)
  process.exit(1)
}
console.log('\n✓ 全部达标')
```

### 10.3 `scripts/check-manual.mjs`

```js
#!/usr/bin/env node
/**
 * 教程内容守护：断言 10 步 / 18 速查 / 22 问，且 Dashboard 与 Manual 同源。
 * 用法：node scripts/check-manual.mjs
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const DATA = path.join(ROOT, 'src/data/manual.ts')
const DASH = path.join(ROOT, 'src/views/Dashboard.vue')
const MANUAL = path.join(ROOT, 'src/views/Manual.vue')

const EXPECT = { steps: 10, tips: 18, faqs: 22 }
let fail = 0

if (!fs.existsSync(DATA)) {
  console.error('✗ 缺少 src/data/manual.ts，先运行 scripts/extract-manual.mjs')
  process.exit(1)
}

const data = fs.readFileSync(DATA, 'utf8')
// 用 [ 	]* 而非 \s*：\s 会吞换行，导致跨行重复计数；subtitle 里也含 title:，必须锚行首
// 按数组分段计数：整文件数 title 会把 manualSteps 的 10 条也算进去
function sliceExport(src, name) {
  const start = src.indexOf(`export const ${name}`)
  if (start < 0) return ''
  // 类型注解 ManualStep[] 里也有 []，必须从 = 之后再找数组起点
  const eq = src.indexOf('=', start)
  const open = src.indexOf('[', eq)
  let depth = 0
  for (let i = open; i < src.length; i++) {
    if (src[i] === '[') depth++
    else if (src[i] === ']') {
      depth--
      if (depth === 0) return src.slice(open, i + 1)
    }
  }
  return ''
}

const stepsBlock = sliceExport(data, 'manualSteps')
const tipsBlock = sliceExport(data, 'manualTips')
const faqsBlock = sliceExport(data, 'manualFaqs')

const counts = {
  steps: (stepsBlock.match(/^[ 	]*key:[ 	]*'/gm) || []).length,
  tips: (tipsBlock.match(/^[ 	]*title:[ 	]*'/gm) || []).length,
  faqs: (faqsBlock.match(/^[ 	]*q:[ 	]*'/gm) || []).length,
}

Object.entries(EXPECT).forEach(([k, want]) => {
  const got = counts[k]
  const ok = got === want
  if (!ok) fail++
  console.log(`${ok ? '✅' : '❌'} ${k}: ${got} / ${want}`)
})

// 数据层不得出现动作：go/action 依赖组件内部状态，留在 Dashboard
if (/(^|\n)\s*(go|action):/.test(stepsBlock)) {
  console.log('❌ manualSteps 里出现了 go/action 字段（函数不能进数据层）')
  fail++
} else {
  console.log('✅ manualSteps 只含内容字段')
}

// 分组元信息必须与 tips 等长，否则筛选 chip 的计数会对不上
const metaCount = (data.match(/group: '[^']+'/g) || []).length
if (metaCount !== counts.tips) {
  console.log(`❌ TIP_META 有 ${metaCount} 项，与 tips ${counts.tips} 条不一致`)
  fail++
} else {
  console.log('✅ TIP_META 与 tips 等长')
}

// 同源检查：两处都不能再内联定义数据
const dash = fs.existsSync(DASH) ? fs.readFileSync(DASH, 'utf8') : ''
const manual = fs.existsSync(MANUAL) ? fs.readFileSync(MANUAL, 'utf8') : ''

const dashInline = /const (tips|faqs)\s*=\s*\[/.test(dash)
if (dashInline) {
  console.log('❌ Dashboard.vue 仍内联定义 tips/faqs，必须改为从 @/data/manual 导入')
  fail++
} else {
  console.log('✅ Dashboard.vue 已改为同源导入')
}

if (manual && !/from '@\/data\/manual'/.test(manual)) {
  console.log('❌ Manual.vue 未从 @/data/manual 导入')
  fail++
}

process.exit(fail ? 1 : 0)
```

### 10.4 stylelint 配置（`.stylelintrc.json`，新增）

```json
{
  "extends": [],
  "rules": {
    "color-no-hex": [
      true,
      {
        "ignore": ["src/design/tokens.ts", "src/styles/tokens.css"],
        "severity": "warning",
        "message": "色值请用 var(--fm-*)，或加到 tokens.ts"
      }
    ],
    "declaration-property-value-disallowed-list": {
      "border-radius": ["/^(?!0$|0px$|50%$|999px$|var\\(--fm-radius)/"],
      "box-shadow": ["/^(?!none$|var\\(--fm-shadow|inset )/"],
      "transition": ["/box-shadow/"]
    },
    "function-disallowed-list": ["rgba", "rgb"]
  }
}
```

依赖安装：

```bash
npm i -D stylelint stylelint-config-standard
npx stylelint "src/**/*.{css,vue}" --fix
```

> `rgb()/rgba()` 全禁的原因：89 处 `rgba` 是深色模式的漏点——浅色写的 `rgba(0,0,0,.05)` 在深色底上几乎不可见。改用 `color-mix(in srgb, var(--fm-text) 5%, transparent)`，跟着文字色自动反相。

---

## 11 测试（C6 提交）

### 11.1 `e2e/manual.spec.ts`

```ts
import { expect, test } from '@playwright/test'

test.describe('使用说明页：内容零删减', () => {
  test('三组条目数正确', async ({ page }) => {
    await page.goto('/#/manual')
    await expect(page.locator('.fm-step')).toHaveCount(10)
    await expect(page.locator('.fm-tip')).toHaveCount(18)
    await expect(page.locator('.fm-faq .q')).toHaveCount(22)
  })

  test('筛选项与条数一致', async ({ page }) => {
    await page.goto('/#/manual')
    const chips = page.locator('.fm-chip')
    const n = await chips.count()
    expect(n).toBe(9) // 全部 + 8 组（变量/脚本/调试/抓包/压测/自动化/航仔/数据）

    await chips.filter({ hasText: '脚本' }).click()
    const shown = await page.locator('.fm-tip').count()
    expect(shown).toBeGreaterThan(0)
    expect(shown).toBeLessThan(18)

    await chips.filter({ hasText: '全部' }).click()
    await expect(page.locator('.fm-tip')).toHaveCount(18)
  })

  test('问答可展开收起', async ({ page }) => {
    await page.goto('/#/manual')
    const first = page.locator('.fm-faq .q').first()
    await first.click()
    await expect(page.locator('.fm-faq .a')).toHaveCount(0) // 默认展开第一条，点击后收起
    await first.click()
    await expect(page.locator('.fm-faq .a')).toHaveCount(1)
  })
})

test.describe('导航与路由未减少', () => {
  test('侧栏 13 项，使用说明存在', async ({ page }) => {
    await page.goto('/#/')
    await expect(page.locator('.nav-item, .fm-nav-item')).toHaveCount(13)
    await expect(page.getByText('使用说明')).toBeVisible()
  })

  test('固定路由全部可达（16 条；带 :id 的 4 条见 smoke）', async ({ page }) => {
    const routes = [
      '/', '/projects', '/stress', '/capture', '/runner', '/automation',
      '/notes', '/ai', '/history', '/monitor', '/tools', '/settings', '/manual',
      '/reports', '/ai/logs', '/ai/usage',
    ]
    for (const r of routes) {
      await page.goto(`/#${r}`)
      await page.waitForLoadState('domcontentloaded')
      const html = await page.locator('#app').innerHTML()
      expect(html.length, `路由 ${r} 渲染为空`).toBeGreaterThan(200)
    }
  })
})
```

> `/project/:id`、`/request/:id`、`/report/:id`、`/environment/:projectId` 需要真实数据 ID，放到 `smoke.spec.ts` 里用测试数据构造，此处只覆盖固定路由。

### 11.2 `e2e/theme-matrix.spec.ts`

```ts
import { expect, test } from '@playwright/test'

const THEMES = [
  'paper', 'night', 'ocean', 'graphite', 'forest', 'midnight', 'amber', 'teal',
  'rose', 'clay', 'dusk', 'violet', 'sky', 'mint', 'coral', 'cocoa', 'lime',
  'steel', 'indigo', 'plum', 'pine', 'slate', 'aurora',
]

const PAGES = ['/', '/stress', '/capture', '/settings']

test.describe('23 套主题视觉矩阵', () => {
  for (const theme of THEMES) {
    test(`主题 ${theme}`, async ({ page }) => {
      await page.addInitScript((t) => {
        localStorage.setItem('astroman.theme', t)
      }, theme)

      for (const p of PAGES) {
        await page.goto(`/#${p}`)
        await page.waitForTimeout(300)
        // 动态区域遮罩：时间、随机读数、实时计数器
        await page.screenshot({
          path: `e2e/theme-matrix/${theme}${p.replace(/\//g, '_') || '_home'}.png`,
          mask: [
            page.locator('.fm-statusbar'),
            page.locator('[data-dynamic]'),
          ],
          maxDiffPixelRatio: 0.02,
        })
      }

      // 关键断言：主色变量已写入
      const primary = await page.evaluate(() =>
        getComputedStyle(document.documentElement).getPropertyValue('--fm-primary').trim(),
      )
      expect(primary).toMatch(/^#[0-9A-Fa-f]{6}$/)
    })
  }
})
```

### 11.3 视觉基线重录流程

```bash
# 1. 改造前确认基线全绿（在 main 分支）
npm run e2e:visual

# 2. C1 结束：只改 token，先肉眼对比，不急着重录
npm run dev

# 3. 每阶段末重录，且必须人工逐图 review
npm run e2e:update
git diff --stat e2e/visual.spec.ts-snapshots/
# 逐张打开确认：没有布局错位、没有低对比、没有内容被裁

# 4. 若某张只是抗锯齿差异，单独 revert 那张快照，不要整体重录
```

**禁止盲签**：`--update-snapshots` 会把任何变化写成新基线，包括 bug。必须逐图看过再提交。

---

## 12 提交级任务清单（照此执行）

每个提交都可独立构建、独立回滚。**按顺序做，不要并行 C1 与 C3**——C1 未完成时改视图，样式会被 `main.css` 的 `!important` 压住，看不到变化。

### C0 · 令牌与门禁（0.5 天，无视觉变化）✅ 已完成 2026-11-09

| 步骤 | 操作 |
|---|---|
| 1 | 新建 `src/design/tokens.ts`（§4.1） |
| 2 | 新建 `src/design/validate.ts`（§4.3） |
| 3 | 新建 `src/styles/tokens.css`（§4.2） |
| 4 | `main.ts` 加 `import './styles/tokens.css'`，**放在 `main.css` 之后** |
| 5 | **删除 `src/styles/main.css` 第 2–60 行的整个 `:root {}` 块**（50 个声明），令牌自此只有 `tokens.css` 一处定义 |
| 6 | 新建 `scripts/check-hardcoded-color.mjs`、`scripts/check-contrast.mjs`、`scripts/check-manual.mjs`（§10；硬编码门禁需排除 `tokens.css`） |
| 7 | `package.json` 加脚本：`"ui:check": "node scripts/check-hardcoded-color.mjs && node scripts/check-contrast.mjs"` |

**验收**

```bash
npm run build                 # 通过
node scripts/check-contrast.mjs   # 全部 ✅
node scripts/check-hardcoded-color.mjs  # 数量 = 基线（hex 425，tokens.css 已排除）
```

再做一次目视确认：删 `:root` 后界面应**看不出变化**（颜色由 `theme.ts` 内联提供，形态变量由 `tokens.css` 提供）。若出现某处变量失效，说明有未声明的 `--fm-*` 被引用，先补齐再继续。

**回滚**：还原 `main.css` 的 `:root` 并移除 `tokens.css` 与 import 即可；本阶段无行为变化。

### C1 · 全局样式与 AntD 接管（1 天，视觉变化 80%）✅ 已完成 2026-11-09

| 步骤 | 操作 |
|---|---|
| 1 | `main.css` 替换 28 条 AntD 覆盖（§5.1） |
| 2 | `main.css` 平台段圆角三处（§5.2） |
| 3 | `main.css` 其余段落（§5.3）：`.page`、`.page-title` 衬线、`fm-fade-in` |
| 4 | `App.vue` 替换 `theme` computed（§5.4） |
| 5 | `theme.ts` 的 `applyTheme` 补 `dataset.palette`（§5.5） |
| 6 | `main.ts` 加 dev 自检调用（同时补 `import { themes } from './stores/theme'`——现有 `main.ts` 只导入了 `initTheme`） |

**验收**

```bash
npm run dev
# 目视检查清单：
# ① 主按钮是纯色实心，无渐变无光晕
# ② 输入框边界清晰可见（对比纸色底）
# ③ 所有卡片无投影，只有 1px 描边
# ④ 页面标题是宋体，字重 600
# ⑤ 圆角统一为 3px（mac 上按钮不再是 7px）
# ⑥ 侧栏无变化（C2 才动）
npm run build
```

**回滚**：`git revert` 单个提交。因为只改了 CSS 变量与 token 映射，无结构变化。

### C2 · 组件层（1.5 天）✅ 文件已落地，视图套用待 C3

| 步骤 | 操作 |
|---|---|
| 1 | 新建 `src/styles/components.css`（§7 全文） |
| 2 | `main.ts` 加 `import './styles/components.css'` |
| 3 | 建组件展示页 `src/views/_DevComponents.vue`（临时路由 `/dev/components`，合并前删除或加 `import.meta.env.DEV` 守卫） |

**验收**：展示页逐项对照设计稿——按钮 5 态、输入 3 态、表格、面板、步骤行、速查卡、路径条、KPI、空状态。

**回滚**：删除 `components.css` 与 import，回到 C1 状态。

### C3 · 视图清理（5 天，可分 4 批提交）✅ 已完成 2026-11-10

| 批次 | 视图 | 说明 |
|---|---|---|
| C3-1 | `Projects` `Reports` `History` `Tools` `QuickCapture` | 低风险，先建立替换手感 |
| C3-2 | `Environment` `Runner` `Automation` `Notes` `AiToolLogs` `AiUsage` `SystemMonitor` | 中等 |
| C3-3 | `Dashboard` `Project` `StressTest` `Settings` | 中高，含布局调整 |
| C3-4 | `RequestEditor` `ReportDetail` `Capture` `AiAssistant` | 高风险，单独回归 |

> **C3-3 工作台落地记录（2026-11-10）**：按设计初稿图 01 重排，不再照 §8.2 的「去投影 + 换圆角」逐项微调。首屏结构：`.console`（1fr / 520px，26px 图纸网格 + 左渐隐）→ 4 宫格 KPI（30px 等宽数）→ `.cols`（最近项目表 + 今日待办）→ 4 个入口一行 → 十格上手路径 → 28px 状态条；原「测试场景 / 批量运行 / 抓包 / 产品介绍 / 功能能力 / 使用说明 / 常用操作」全部收进默认折叠的 `.guide`。新增只读数据调用与跨页 `PageHeader` 改动的偏差见文首「落地偏差」7–11。
>
> **C3 四批完成记录（2026-11-10）**：C3-1/C3-2 完成 12 个中低风险视图；C3-3 完成 `Project`/`StressTest`/`Settings`（工作台见上）；C3-4 完成 `RequestEditor`/`ReportDetail`/`Capture`/`AiAssistant`。四批均以令牌替换、圆角/投影清理、状态色与等宽数字为主，未重写业务逻辑；结构性差异（QuickCapture 尺寸、Runner 编号面板、Capture 两栏、ReportDetail 局部暗色）见文首「落地偏差」12–18。门禁实测 hex 239 / rgba 20 / radius 447 / shadow 51 / deep 110，基线已同步下调。

每批执行：

```bash
# 1. 批量替换（§8.1）
# 2. 逐文件 git diff 检查
git diff src/views/<file>.vue

# 3. 该批视图逐个打开验收
# 4. 跑门禁
node scripts/check-hardcoded-color.mjs
npm run build && npm test
```

**回滚**：按批次 revert，不影响其他批次。

### C4 · 使用说明页（1.5 天，依赖 C2）

| 步骤 | 操作 |
|---|---|
| 1 | `node scripts/extract-manual.mjs` 生成 `src/data/manual.ts` |
| 2 | `Dashboard.vue` 改为同源导入（模板不动） |
| 3 | 新建 `src/composables/useManualProgress.ts` |
| 4 | 新建 `src/views/Manual.vue` |
| 5 | 新建 `src/components/manual/PathBand.vue` 并接入 Dashboard |
| 6 | `router/index.ts` 加 `/manual`，`App.vue` 的 `navs` 加一项 |

**验收**

```bash
npm run build                     # 主要关卡：manual.ts 的类型与字段必须对得上
node scripts/check-manual.mjs     # 10 / 18 / 22 全 ✅，且 manualSteps 无 go/action、TIP_META 与 tips 等长
npm run e2e:smoke
# 目视：侧栏 13 项；/manual 三块内容齐全；工作台底部路径条未被裁切
```

> `check-manual.mjs` 只数条数，查不出类型错误。**C4 的通过条件是 `npm run build` 绿**，不是门禁绿。

**回滚**：删路由与导航项即可；`src/data/manual.ts` 保留（Dashboard 已在用，不影响）。

### C5 · 主题重映射（1.5 天）✅ 已完成 2026-11-10

| 步骤 | 操作 | 状态 |
|---|---|---|
| 1 | 写 `scripts/theme-migrate.mjs`（§6.3） | ✅（仅解析 21 套旧主题，`swatches` 按新色组派生） |
| 2 | `node scripts/theme-migrate.mjs --check` 确认 21 套达标 | ✅ 21/21 通过 |
| 3 | 生成并替换进 `theme.ts` | ✅ 已替换，`theme.generated.ts` 已删除 |
| 4 | 加 `paper` / `night` 两套，`ThemeId` 扩展，`loadThemeId()` 兜底改 `'paper'` | ✅ `findTheme()` 兜底同步改为 `paper` |
| 5 | `themes` 数组把 `paper` 放首位 | ✅（`paper`、`night`、`ocean`… 共 23 套） |

**验收**

```bash
node scripts/check-contrast.mjs        # 12 对 × 2 组全绿
node scripts/theme-migrate.mjs --check # 21 套全绿
npm run build                          # vue-tsc + vite 绿
npm run e2e:visual                     # 基线为换皮前版本，按设计会红，待 C6 重录
```

**回滚**：`git checkout main -- src/stores/theme.ts`（主题文件独立，不改其他文件）。

### C6 · 基线重录与收尾（1 天）

| 步骤 | 操作 |
|---|---|
| 1 | 新建 `src/styles/print.css` 与 `@media print` 规则 |
| 2 | `npm run e2e:update` 重录基线，逐图 review |
| 3 | 跑 `e2e/manual.spec.ts`、`e2e/theme-matrix.spec.ts` |
| 4 | 删除临时展示页与 `theme.generated.ts` |
| 5 | 更新 `README.md` 的设计系统章节（如有） |

**验收**：§14 全部勾选。

---

## 13 风险登记与应对

| # | 风险 | 概率 | 影响 | 具体应对 |
|---|---|---|---|---|
| 1 | 425 处硬编码色值在深色主题下不可读 | 高 | 高 | lint 拦新增；23 套 × 4 页矩阵截图；优先清 `src/views` 里 `background: #fff` 这类硬底 |
| 2 | AntD 与自绘控件风格撕裂 | 中 | 高 | §5.4 的 27 组 `components` 映射；C1 结束即可肉眼验证 |
| 3 | `RequestEditor` 布局回归 | 中 | 高 | 只改样式；三处高度用 `flex: 0 0 282px` 而非百分比；行号与代码同 `line-height` |
| 4 | `controlHeight` 34→30 造成 20 页错位 | 高 | 中 | **本次不改**，留 P2 专项；非要改则先改 AntD token 跑一遍回归再改自绘 |
| 5 | 视觉基线大面积失效 | 高 | 中 | 每阶段末重录 + 人工逐图 review；动态区域 `mask` |
| 6 | 21 套主题映射后「千篇一律」 | 中 | 中 | 只改 L/C 不改 H；设置页 `swatches` 仍是 4 色，差异可见 |
| 7 | 宋体标题在低分屏发虚 | 中 | 低 | 子集化 + `font-display: swap`；<1100px 时标题回退无衬线 |
| 8 | 包体积增加 | 低 | 低 | 只内嵌 1 个字体子集（≤150KB），不引图标库 |
| 9 | `dataset.palette` 与旧 CSS 冲突 | 低 | 中 | 全局搜索 `data-theme=`，确认无样式依赖明暗判断 |
| 10 | `sed` 批量替换误伤 SVG `fill` | 中 | 中 | 替换前 `git checkout -b`，逐文件 diff；SVG 单独处理 |

### 13.1 字体子集化（只在决定内置宋体时做）

```bash
pip install fonttools brotli
pyftsubset SourceHanSerifCN-SemiBold.otf \
  --text-file=./scripts/gb2312.txt \
  --flavor=woff2 \
  --output-file=src/assets/fonts/serif-title.woff2
```

`src/styles/tokens.css` 追加：

```css
@font-face {
  font-family: 'AstroSerif';
  src: url('@/assets/fonts/serif-title.woff2') format('woff2');
  font-weight: 600;
  font-display: swap;
}
```

并把 `--fm-font-serif` 的首选改为 `'AstroSerif'`。

---

## 14 验收清单（DoD）

**构建与测试**

- [ ] `npm run build`（含 `vue-tsc --noEmit`）通过，无新增类型错误
- [ ] `npm test` 全绿
- [ ] `npm run e2e:smoke` 全绿
- [ ] `npm run e2e:visual` 全绿，基线已重录并人工确认
- [ ] `node scripts/check-contrast.mjs` 全部 ✅
- [ ] `node scripts/check-manual.mjs` 输出 10 / 18 / 22 全 ✅

**视觉**

- [ ] 主按钮纯色，全站无渐变按钮、无彩色光晕
- [ ] 全站卡片零投影（模态除外）
- [ ] 圆角统一 3px（含 mac / win 两平台）
- [ ] 页面标题宋体，正文无衬线，数字等宽
- [ ] 23 套主题 × 4 关键页截图无低对比、无错位
- [ ] 侧栏选中用 2px 关注线，不是整块色块

**功能零删减**

- [ ] 路由表 20 个视图 + catch-all，一条不少
- [ ] 侧栏 13 项（原 12 + 使用说明）
- [ ] 教程 10 步 / 18 速查 / 22 问，Dashboard 与 Manual 同源
- [ ] 21 套旧主题 id、名称、`swatches` 全部保留
- [ ] 所有按钮、字段、Tab、快捷键原位
- [ ] `localStorage` 键名未变（`astroman.theme`、`fm-zoom`、`astroman:sidebar-collapsed`）

**工程质量**

- [ ] 硬编码：hex ≤40、rgba ≤10、radius ≤120、shadow ≤20
- [ ] `:deep(.ant-*)` 从 110 降至 ≤60
- [ ] 新增 `!important` 数量为 0
- [ ] 亮暗切换无闪烁；启动首帧即为已保存主题

**可访问性**

- [ ] 正文对比度全部 ≥4.5，非文字 ≥1.5
- [ ] 键盘可完成：切主题、开手册、筛速查、展开问答
- [ ] `prefers-reduced-motion` 下无动效
- [ ] 状态不只靠颜色（方法名、PASS/FAIL、✓/✗ 都有文字）

**打印**

- [ ] 打印/导出 PDF 不出现侧栏、不出现深色底
- [ ] 表格不被跨页切断（`break-inside: avoid`）

---

## 15 实测验证记录（本文脚本均已跑过）

本文中的令牌文件与 4 个门禁脚本，在交付前已在 `D:\Software\ProjectFiles\AstroMan` 的当前工作区副本上实际执行过，不是纸面代码。以下为真实输出与过程中修正的问题。

### 15.1 执行结果

| 验证项 | 命令 | 实际输出 |
|---|---|---|
| 主题迁移自检 | `node scripts/theme-migrate.mjs --check` | 解析 21 套，逐套输出「旧主色 → 新主色 / 主色对比度 / 全套最差」，全部 ✅，末行 `✓ 21 套主题全部达标` |
| 对比度门禁 | `node scripts/check-contrast.mjs` | 宣纸 12 对、夜航舱 12 对全 ✅；关键值：`text/surface 17.10`、`primary/surface 6.08`、`muted/surface-alt 4.63`、`border-control/surface 1.70`、深色 `text 14.48`、`primary 7.67` |
| 硬编码门禁 | `node scripts/check-hardcoded-color.mjs` | `hex 425 / rgba 89 / radius 453 / shadow 73 / deep 110`（口径**不含** `tokens.css`），与基线一致，`✓ 硬编码数量在基线内`；并打印前 15 个高密度文件（`RequestEditor.vue` 24/26/40/11/21 居首） |
| 教程抽取 | `node scripts/extract-manual.mjs` | `✓ 已写入 src/data/manual.ts（steps 10 / tips 18 / faqs 22）` |
| 教程门禁 | `node scripts/check-manual.mjs` | `✅ steps 10/10  ✅ tips 18/18  ✅ faqs 22/22  ✅ Dashboard.vue 已改为同源导入` |
| 令牌类型与派生 | `tsc --strict` + 运行时断言 | 编译通过（仅 `node_modules/@types/markdown-it` 的既有类型错误）；`COLOR_KEYS 38`、`paper 38 键`、`night 38 键`、`makeVars` 亮/暗各 38 键、无空值 |

### 15.2 验证中发现并已修正的 5 个问题

| # | 问题 | 表现 | 修正 |
|---|---|---|---|
| 1 | 迁移脚本正则不兼容 CRLF | 在 Windows 工作区解析到 **0 套**主题，直接退出 | 正则改为 `
?
`，并加注释说明原因 |
| 2 | 深色发丝线对比度不足 | `--fm-border` `#242C33` 对面板底仅 **1.27**，低于 1.3 可见阈值，深色下分隔线几乎消失 | 改为 `#2C353D`，三档背景分别为 1.45 / 1.36 / 1.51，与亮色 1.39 对称，且仍低于 `border-strong` 的 1.70 |
| 3 | 硬编码基线口径不一致 | 文档写 378，脚本实测 425——原数字未计入 `src/styles` | 统一为含 `src/styles` 的口径 425/89/453/73/110，并在两处标注统计范围 |
| 4 | 教程计数把 `subtitle` 算成 `title`，且 `\s*` 跨行重复计数 | `tips` 报 28 而非 18 | 改为锚行首 + `[ 	]*` 限定行内；并修正「整文件数 title 会把 steps 的 10 条也算进去」，改为按数组分段 |
| 5 | 数组截取被类型注解干扰 | `ManualStep[]` 里的 `[]` 让截取立即结束，三段计数全为 0 | 从 `=` 之后再定位数组起点 |

> 这正是「AI 拿到就能开发」与「一份看起来详细的文档」的差别：上面 5 个问题若不在交付前跑出来，接手的人会在第一步就卡住，而且很难判断是脚本错还是环境错。

> **关于 v2.1**：本节记录的是 v2.0 交付时的实跑结果。v2.1 的四处修订（C0 删 `:root`、`c()` 取色器、门禁排除 `tokens.css`、C4 内容/动作分离）**没有重跑全部脚本**——其中 C4 的抽取脚本已按 `Dashboard.vue` 的真实字段重写，但未生成过产物。执行时以 §12 每个提交的验收命令为准；任何一条不绿都不要进入下一阶段。

## 附录 A 令牌与旧值对照（迁移速查）

| 令牌 | 旧值（ocean） | 新值（paper） | 归属 |
|---|---|---|---|
| `--fm-primary` | `#4d6bfe` | `#0F6B64` | 主题 |
| `--fm-primary-dark` | `#3b53e6` | `#0B524D` | 主题 |
| `--fm-primary-soft` | `#eef1ff` | `#E4EDEA` | 主题 |
| `--fm-success` | `#16a34a` | `#2A7253` | 主题 |
| `--fm-success-soft` | `#e8f8ee` | `#E7F0EB` | 主题 |
| `--fm-warning` | `#d97706` | `#8F6118` | 主题 |
| `--fm-warning-soft` | `#fdf3e2` | `#F6EEE0` | 主题 |
| `--fm-danger` | `#dc2626` | `#A83A2C` | 主题 |
| `--fm-danger-soft` | `#fdeeee` | `#F6E7E4` | 主题 |
| `--fm-purple` | `#8b5cf6` | `#6A4A87` | 主题 |
| `--fm-purple-soft` | `#f4f0fe` | `#EFEAF4` | 主题 |
| `--fm-bg` | `#f5f6fa` | `#F3F1EC` | 主题 |
| `--fm-surface` | `#ffffff` | `#FBFAF7` | 主题 |
| `--fm-surface-alt` | `#f2f4f9` | `#F0EEE8` | 主题 |
| `--fm-border` | `#e6e9f2` | `#DBD6CB` | 主题 |
| `--fm-border-strong` | `#ccd3e4` | `#C9C2B5` | 主题 |
| `--fm-border-control` | **无** | `#C9C2B5` | 主题（新增） |
| `--fm-text` | `#151b28` | `#14181B` | 主题 |
| `--fm-text-2` | `#3a4256` | `#39424A` | 主题 |
| `--fm-muted` | `#6b7490` | `#616C74` | 主题 |
| `--fm-sidebar` | `#f7f8fc` | `#F7F5F0` | 主题 |
| `--fm-sidebar-hover` | `#edf0f8` | `#EFEDE6` | 主题 |
| `--fm-sidebar-active` | `#e9edfe` | `#E4EDEA` | 主题 |
| `--fm-sidebar-text` | `#5f6b85` | `#4E585F` | 主题 |
| `--fm-sidebar-icon` | `#7b87a3` | `#6C777E` | 主题 |
| `--fm-sidebar-active-text` | `#4d6bfe` | `#0F6B64` | 主题 |
| `--fm-code-bg` | `#fafbff` | `#FBFAF7` | 主题 |
| `--fm-gutter-bg` | `#f4f6fb` | `#F0EEE8` | 主题 |
| `--fm-json-key` | `#2563eb` | `#2B5D8C` | 主题 |
| `--fm-json-string` | `#16a34a` | `#2A7253` | 主题 |
| `--fm-json-number` | `#d97706` | `#8F6118` | 主题 |
| `--fm-json-bool` | `#9333ea` | `#6A4A87` | 主题 |
| `--fm-json-punct` | `#8b94ad` | `#8B959B` | 主题 |
| `--fm-shadow-sm` | `0 1px 2px rgba(…)` | `none` | 主题 |
| `--fm-shadow` | 双层投影 | `none` | 主题 |
| `--fm-shadow-lg` | `0 12px 28px rgba(…)` | `0 18px 48px rgba(20,24,27,.16)` | 主题 |
| `--fm-brass` | **无** | `#A97C3F` | 主题（新增） |
| `--fm-brass-text` | **无** | `#8A6530` | 主题（新增） |
| `--fm-radius-sm` | `6px` | `2px` | 静态 |
| `--fm-radius` | `8px` | `3px` | 静态 |
| `--fm-radius-lg` | `14px` | `3px` | 静态 |
| `--fm-method-*` | 高饱和 6 组 | 降饱和同色相 6 组 | 静态（不进主题） |

## 附录 B AntD token 映射对照

| token | 现状 | 改后 | 视觉影响 |
|---|---|---|---|
| `colorPrimary` | 主色 | 主色 | — |
| `colorBorder` | `--fm-border`（发丝线）❌ | `--fm-border-control` | 输入框/下拉边界变清楚 |
| `colorBorderSecondary` | 未设 | `--fm-border` | 表格分隔线 |
| `colorTextTertiary` | 未设 | `--fm-muted` | 占位符达标 AA |
| `colorBgElevated` | 主面板色 | 同 | 弹层与面板同色，靠描边分层 |
| `borderRadius` | `isMac ? 7 : 6` | `3` | 全站圆角 |
| `borderRadiusLG` | `isMac ? 11 : 10` | `3` | 卡片、弹窗 |
| `borderRadiusSM` | 未设（6） | `2` | 标签、树节点 |
| `fontFamily` | 平台硬切 | `--fm-font-sans` | 与自绘一致 |
| `boxShadow` | AntD 默认 | `none` | 去「浮起来」 |
| `controlHeight` | `isMac ? 32 : 34` | **不变** | 密度留 P2 |
| `motionDuration*` | 默认 | `0.12/0.18/0.24s` | 节奏统一 |
| `Table.headerBg` | `--fm-surface-alt` | 保持 | — |
| `Table.headerSplitColor` | 未设 | `transparent` | 去表头竖线 |
| `Collapse.headerBg` | 默认 | `transparent` | 问答贴平 |
| `Tabs.inkBarColor` | 默认 | 主色 | — |
| `Tabs.titleFontSize` | 默认 14 | `12.5` | 与正文一致 |

## 附录 C 文件改动清单

| 类型 | 文件 | 阶段 | 说明 |
|---|---|---|---|
| 新增 | `src/design/tokens.ts` | C0 | 38 键语义色 + 静态令牌 + `makeVars` |
| 新增 | `src/design/validate.ts` | C0 | dev 期主题完整性自检 |
| 新增 | `src/styles/tokens.css` | C0 | 静态令牌 + 首帧兜底 + method 色 |
| 新增 | `src/styles/components.css` | C2 | `.fm-*` 组件层 |
| 新增 | `src/styles/print.css` | C6 | 打印与 PDF |
| 新增 | `src/data/manual.ts` | C4 | 10 步 / 18 速查 / 22 问 |
| 新增 | `src/composables/useManualProgress.ts` | C4 | 进度读写 |
| 新增 | `src/views/Manual.vue` | C4 | 使用说明页 |
| 新增 | `src/components/manual/PathBand.vue` | C4 | 上手路径条 |
| 新增 | `scripts/theme-migrate.mjs` | C5 | 21 套重映射（一次性） |
| 新增 | `scripts/extract-manual.mjs` | C4 | 教程数据抽取（一次性） |
| 新增 | `scripts/check-hardcoded-color.mjs` | C0 | 硬编码门禁 |
| 新增 | `scripts/check-contrast.mjs` | C0 | 对比度门禁 |
| 新增 | `scripts/check-manual.mjs` | C0 | 教程条数门禁 |
| 新增 | `e2e/manual.spec.ts` `e2e/theme-matrix.spec.ts` | C6 | 内容与主题矩阵 |
| 新增 | `.stylelintrc.json` | C0 | 样式纪律 |
| 修改 | `src/styles/main.css` | C0/C1 | C0 删除 `:root` 50 个声明；C1 改写 28 条 AntD 覆盖、平台段圆角、`.page` 等 |
| 修改 | `src/stores/theme.ts` | C1/C5 | `applyTheme` 补 palette；21 套重映射；新增 2 套 |
| 修改 | `src/App.vue` | C1/C4 | `theme` computed 完整映射；`navs` 加一项 |
| 修改 | `src/router/index.ts` | C4 | 加 `/manual` |
| 修改 | `src/main.ts` | C0/C2 | 引入 tokens / components；dev 自检 |
| 修改 | `src/views/*.vue`（20 个） | C3 | 仅 `<style>` 与 class |
| 不动 | `src-tauri/`、`src/api/`、`src/stores/*`（除 theme） | — | 后端与数据层零改动 |

## 附录 D 与设计稿的差异（需设计确认的 4 处）

| # | 设计稿 | 实施方案 | 原因 |
|---|---|---|---|
| 1 | 青铜色只有一支 `#A97C3F` | 拆成 `--fm-brass`（装饰）与 `--fm-brass-text`（`#8A6530`） | 小字号下 `#A97C3F` 对比度 3.57，不到 4.5 |
| 2 | `muted` 为 `#6C777E` | 改为 `#616C74` | 4.40 → 4.63，跨过 AA 线，肉眼几乎无差 |
| 3 | 控件密度 30px | 保持 32/34，P2 再统一 | 一次性改会让 20 个视图同时错位，回归量大于收益 |
| 4 | 使用说明独立成页 | 独立成页 + 工作台保留入口与折叠区块 | 信息架构微调；数据同源，否决时删路由即回滚 |

## 附录 E 开工前检查（5 分钟）

```bash
cd /d/Software/ProjectFiles/AstroMan

# 1. 工作区干净
git status --short

# 2. 开分支
git checkout -b ui-redesign

# 3. 基线数字记录下来，改造后对比
node -e "
const {execSync}=require('child_process');
const c=(cmd)=>execSync(cmd,{encoding:'utf8'}).trim();
console.log('hex   ', c('grep -roh \\'#[0-9a-fA-F]\\\\{3,8\\\\}\\\\b\\' src/views src/components src/App.vue | wc -l'));
"

# 4. 确认能构建
npm run build

# 5. 确认视觉基线当前状态（只记录，不重录）
npm run e2e:visual || echo '基线当前不绿，先查清原因再动手'
```

> 第 5 步若基线本来就不绿，说明改造前已有视觉漂移。**先修好再开工**，否则后面分不清是谁引入的。
