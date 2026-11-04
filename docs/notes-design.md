# 记事本与待办 · 技术设计

> 状态：已实现（v1.0.9） · 2026-11-04
> 范围：记事、待办完成、截止时间、到期提醒、重复提醒、稍后提醒、截图转待办与图片附件，以及面向日常使用的快速记录、全局快捷键、免打扰与撤销；本文件只做设计，不含实现代码。

## 1. 背景与目标

AstroMan 目前覆盖接口调试、批量回归、压测、抓包和离线工具箱，但日常使用中「记一件事、到点提醒、做完打勾」没有落点。目标是在应用内提供一个本地优先的记事本，并优先服务日常高频路径：

- **3 秒记下来**：全局快捷键唤起迷你窗口，输入一句话回车即存，不打断当前工作。
- **打开就知道今天干什么**：默认视图直接展示逾期 + 今天，列表按时间分组，不需要先选筛选条件。
- **到点有人管，但只打扰一次**：应用内提醒卡片，多条聚合；支持稍后提醒、暂停提醒、免打扰时段。
- **误操作能撤回**：完成和删除都有撤销入口，删除保留 30 天可恢复。
- **截图直接变待办**：粘贴或选择一张截图，识别出待办清单，确认后批量生成；原图挂在每条待办上，随时点开查看。
- **本地优先**：数据只存本机 SQLite，随备份归档，不引入账号或云同步；只有主动点「识别」时图片才发送给已配置的 AI 服务商。

### 非目标（v1 不做）

- 富文本与 Markdown 渲染，正文按纯文本保存与展示；图片仅作为附件，不嵌入正文排版。
- 离线 OCR / 手写识别：截图识别依赖已配置的视觉模型，不内置 OCR 运行时。
- 多设备同步、团队协作、账号体系。
- 系统日历双向同步。
- 子任务、多级清单、看板。
- 应用完全退出后的提醒（不注册 Windows 计划任务 / 开机自启）。
- 使用 Windows / macOS 系统通知：v1 只走应用内提醒卡片，避免开发模式出现 PowerShell 署名与双份提醒。
- 通知中心历史、免打扰白名单、按条目自定义声音。

## 2. 现状与可复用能力

| 事项 | 现状 | 对本功能的影响 |
|---|---|---|
| `notes` 表 | 已在 `src-tauri/src/db/schema.sql` 中创建，字段覆盖截止、提醒、提前量、重复、稍后、通知时间、置顶 | 直接复用，补 `done_at` / `deleted_at` / `deleted_from_status` 与索引 |
| 数据目录 | `AppState.data_dir` + `astroman.db`，单实例运行 | 与项目、历史、报告同库，备份自动覆盖 |
| 备份 | `VACUUM INTO` 整库快照 | `notes` 与图片 BLOB 自动进入备份与诊断导出，无需改备份逻辑 |
| AI 服务商 | `ai_providers` 已支持 openai / anthropic / gemini 三类，含加密 Key 与用量统计；现有 `ai_chat` 只发送纯文本，且没有记录模型是否支持图片 | 截图识别新增一条独立的视觉调用路径，不改动聊天协议；复用已配置的服务商与用量记录；新增 `supports_vision` 标记，用于筛选可识别图片的模型 |
| 窗口关闭 | `lib.rs` 拦截 `CloseRequested`，隐藏到托盘；只有托盘「退出」才结束进程 | 隐藏后后台调度器仍在运行，提醒可持续触发 |
| 事件通道 | 已有 `stress:progress`、`backup://progress` 等 `app.emit` + 前端 `listen` 模式 | 新增 `notes:reminder`、`notes:changed` 事件 |
| 托盘 | `main-tray` 已存在，支持 tooltip 与菜单 | 增加「快速记录」「暂停提醒」「打开记事本」入口 |
| 通知 / 全局快捷键 | 均未安装 | 提醒使用前端 Ant Design Vue 卡片，不引入系统通知插件；全局快捷键新增 `tauri-plugin-global-shortcut` |

## 3. 产品形态

### 3.1 入口

| 入口 | 行为 |
|---|---|
| 侧边栏「记事本」 | 进入 `/notes`，角标显示「逾期 + 今天到期」数量，超过 99 显示 `99+` |
| 工作台「今日待办」卡片 | 展示逾期 / 今天数量、前 5 条、快速记录输入框 |
| 全局快捷键 `Ctrl+Alt+N` | 在任何应用前台时唤起迷你快速记录窗口；可在设置中关闭 |
| 托盘菜单 | 「快速记录」「暂停提醒」「打开记事本」；tooltip 显示最近一条待办 |
| 应用内提醒卡片 | 到期时在右下角弹出，可直接完成、稍后 10 分钟或打开记事本 |
| 记事本工具栏「截图生成」 | 粘贴 / 拖入 / 选择截图，识别后预览待办清单，确认再批量创建 |
| 编辑器粘贴图片 | `Ctrl+V` 或拖拽图片直接作为附件挂到当前条目，不强制识别 |

### 3.2 页面布局

```text
┌─────────────────────────────────────────────────────────────────────┐
│ 记事本          [截图生成] [⏸ 提醒] [🔔] [⚙] [+ 新建▾] [搜索…]     │
├──────────────┬──────────────────────────────────────────┬───────────┐
│ 今天    3    │ 记一笔… 明天10点 交周报 #工作             │ 标题      │
│ 本周    7    │ ── 逾期 1 ─────────────────────────────── │ 待办|笔记 │
│ 全部    12   │ ☐ 续费域名        ⚠ 已逾期               │ 灵感|资料 │
│ 已完成  24   │   昨天 09:00 · 每天重复  📎 1             │ 正文…     │
│              │ ── 今天 2 ─────────────────────────────── │ 附件 [+]  │
│ 类型         │ ☐ 提交周报                                │ 截止…     │
│ 全部类型     │   今天 18:00 · 提前 15 分钟               │ 提醒…     │
│ 待办/笔记…   │ ▸ 💡 记录：网关排查结论                   │ [删除][保存]│
│ 灵感/资料    │   无截止时间                              │           │
│ 标签         │                                           │  抽屉 50% │
│ #工作 #个人  │                                           │           │
└──────────────┴──────────────────────────────────────────┴───────────┘
```

- 左栏：今天 / 本周 / 全部 / 已完成，下面是「类型」（全部类型 + 待办 / 笔记 / 灵感 / 资料）与标签筛选及数量；「标签」标题右侧有「+」，点开后在原地输入名称回车即可新建空标签，未使用的标签置灰显示，方便先建分类再往里记。类型与视图、标签是 AND 关系，可叠加过滤。
- 标签整块着色：标签名哈希到 8 组主题内可读的胶囊配色（`notesTags.ts` 的 `tagTone`），同一标签在左栏、列表、卡片、编辑器里颜色一致；未使用的标签保留中性灰，选中时加重描边。
- 中栏：顶部常驻「记一笔」输入框；下面按「逾期 / 今天 / 之后 / 无截止」分组，吸顶分组标题。行标题前常驻类型图标（待办 ☑ / 笔记 📄 / 灵感 💡 / 资料 🔗），用主题内低饱和色区分。
- 中栏「记一笔」预览行右侧提供列表 / 卡片切换：列表保持紧凑单行，卡片改为自适应网格（每张卡片显示标题、截止、重复、优先级、附件数与摘要），两种模式的选择记在 `localStorage`。
- 卡片配色沿用主题色：默认白卡 + 细边 + 轻阴影，选中为淡主色渐变 + 主色边框，逾期为淡红渐变 + 红色左边条，已完成为灰底，保证与列表视图的优先级 / 逾期语义一致。
- 整体是笔记本质感：页面底部点纸纹理，面板顶部压一层主题色渐变；编辑器像活页纸——标题是虚线下划线，正文是横线格（`repeating-linear-gradient`），左侧一条页边线；分组标题带彩色横条与数量药丸，避免整页只有黑白文字。
- 详情 / 编辑统一走右侧抽屉：点击条目才打开，默认占窗口 50% 宽度（窄窗口下自动收缩，最小 520px、最大不超过窗口减边距），抽屉内就是完整编辑器；关闭抽屉即回到列表。新建也从同一抽屉进入，不再把编辑器常驻成第三栏。
- 顶部「⏸ 提醒」显示暂停状态；「🔔」快速开关到点提醒；「⚙」打开记事本设置；「+ 新建▾」展开类型菜单（待办 / 笔记 / 灵感 / 资料，各带一句用途说明），选谁就直接建对应类型并打开抽屉。

**记录类型（v1 固定四种）**

| 类型 | 用途 | 默认形态 |
| --- | --- | --- |
| `todo` 待办 | 有明确动作，需要勾选完成，可设截止与提醒 | 默认类型；快速记录不带类型时归到这里 |
| `note` 笔记 | 记录信息、会议、总结，重在正文 | 不强调完成，可不设截止 |
| `idea` 灵感 | 随手记下的想法和点子，允许长期沉淀 | 不强调完成，可不设截止 |
| `reference` 资料 | 链接、命令、配置片段等可复用内容 | 不强调完成，可不设截止 |

类型只影响分组展示、图标与筛选，不改完成 / 删除 / 提醒的数据结构：任何类型都可以设置截止与提醒，也都能勾选完成。历史数据在迁移时统一归一化：老版本写过的 `note` 且带截止时间的条目改成 `todo`，未知值一律按 `todo` 处理，避免出现筛选不到的孤儿记录。

### 3.3 默认视图与分组

- 默认进入「今天」，条件为 `status = 'open' AND (due_at IS NULL OR due_at < 明天 00:00)`，**包含所有逾期条目和无截止条目**，避免昨天的任务、以及随手记下的一句话被藏起来。
- 「本周」条件为 `status = 'open' AND (due_at IS NULL OR due_at < 下周一 00:00)`，同样包含逾期与无截止。
- 分组顺序固定：逾期 → 今天 → 之后 → 无截止，空分组不渲染。无截止条目在「今天」「本周」「全部」里都常驻最后一组，保证快速记录不带日期的内容立刻可见。
- 组内排序：置顶 → 优先级高 → 截止时间升序 → 更新时间倒序。
- 为什么无截止要进「今天」：快速记录的主路径允许不带时间，如果默认视图只显示有截止的条目，用户记完一句话会以为没保存成功。
- 已完成视图按 `done_at DESC`，每条显示完成时间，可恢复或删除。

### 3.4 快速记录

#### 主窗口「记一笔」

- 输入框常驻列表顶部，占位提示：`试试：明天10点 交周报 #工作 !`。
- 回车保存，保存后保持焦点，方便连续录入。
- 不带日期也允许保存，条目会立刻出现在当前视图末尾的「无截止」组，不会因为默认「今天」过滤而消失。
- 粘贴多行文本时：首行作为标题，其余作为正文。
- 输入内容按自然语言解析，保存前在输入框下方显示预览标签：`截止 明天 10:00`、`每周`、`#工作`、`高优先级`。
- 解析只提取能确定的部分，剩余文字原样作为标题；不认识的写法不会被猜测。

#### 迷你快速记录窗口

- 全局快捷键 `Ctrl+Alt+N` 唤起，窗口约 520×200，置顶、无边框、不占任务栏，居中显示。
- 结构：一行输入 + 预览标签 + 一行提示；回车保存并隐藏，`Esc` 隐藏不保存。
- 草稿保存在 `localStorage`，`Esc` 后再次唤起仍能看到未提交内容。
- 保存后通过 `notes:changed` 事件通知主窗口刷新列表、角标和工作台卡片。
- 首次使用在窗口内提示全局快捷键，可关闭。

#### 自然语言解析规则

解析在前端完成，纯本地、无网络请求，规则保守：

| 类型 | 支持写法 | 结果 |
|---|---|---|
| 相对日期 | 今天 / 今日 / 明天 / 明日 / 后天 / 大后天 | 对应日期；只给日期时默认 18:00 |
| 星期 | 周一~周日、周天、星期X、下周一~下周日 | 下一次该星期；只给星期时默认 18:00 |
| 具体日期 | 11月5日、11月5号、2026-11-05 | 对应日期；只给日期时默认 18:00 |
| 相对月份 | 下月3号 | 下个月 3 日 18:00；该月无此日则取最后一天 |
| 时刻 | 10点、10:30、上午9点、下午3点、晚上8点、中午12点 | 结合日期；没有日期时按今天 |
| 重复 | 每天 / 每日 / 每周X / 每月X号 / 工作日 | 设置重复规则；重复与截止时间一起写入 |
| 标签 | `#工作`、`＃工作` | 标签，最多 20 个 |
| 优先级 | `!`（高）、`!!`（紧急→高） | 优先级高 |

示例：

| 输入 | 解析结果 |
|---|---|
| `明天10点 交周报 #工作` | 标题「交周报」，截止明天 10:00，标签工作 |
| `每周一 9:30 站会 #工作` | 标题「站会」，下一次周一 09:30，每周重复 |
| `今晚8点 取快递` | 标题「取快递」，今天 20:00 |
| `下月3号 续费域名 !` | 标题「续费域名」，下月 3 日 18:00，高优先级 |
| `工作日 9点 看邮件` | 标题「看邮件」，下一次工作日 09:00，工作日重复 |
| `网关报错排查结论` | 标题原文，无截止时间 |

- 只出现重复词（每天 / 工作日 / 每周X / 每月X号）而没有明确日期时，首次截止取「下一次未来时刻」：每天取今天或明天，工作日取下一个工作日，每周X取下一次该星期，每月X号取下一次该日期，避免刚保存就变成逾期。
- 明确写了「今天 10 点」这类已经过去的时间时，不自动顺延，而是显示 `今天 10:00（已过）`，保存后计入逾期，让用户自己判断。
- 只输入时间词、没有标题时拒绝保存，提示「再写点内容」。
- 解析结果可在保存后进入编辑器调整；v1 不做保存前的逐项编辑弹层。

### 3.5 日常操作与键盘流

| 场景 | 操作 |
|---|---|
| 快速记录 | 主窗口 `N` 或在输入框直接输入；全局 `Ctrl+Alt+N` 唤起迷你窗口 |
| 搜索 | `/` 聚焦搜索框，`Esc` 退出 |
| 列表导航 | `↑` / `↓` 或 `J` / `K` 切换条目 |
| 完成 | `Space` 切换选中条目完成状态 |
| 编辑 | `Enter` 打开右侧抽屉编辑；`Ctrl/Cmd + S` 立即保存；`Esc` 收起抽屉 |
| 稍后提醒 | 列表行悬停出现铃铛按钮，默认 10 分钟；通知内可选 10 / 30 / 60；仅对已到期（含逾期）或已进入提前量窗口的条目可用 |
| 置顶 / 删除 | 行悬停按钮；删除后 6 秒内可撤销 |
| 批量清理 | 已完成视图「清空已完成」，二次确认 |

- 行悬停操作：`完成`（复选框常驻）、`稍后 10 分钟`（未到期时禁用）、`置顶`、`删除`。
- 页面级快捷键统一经过 `isEditableTarget(event.target)` 守卫：焦点在 input / textarea / contenteditable / 下拉与日期选择器内时，`N`、`/`、`J`、`K`、`Space` 一律不触发；`Space` 需要 `preventDefault` 避免滚动页面。
- 完成一次性条目后弹 6 秒撤销提示；重复条目完成后提示「已顺延到 X」，不提供撤销。
- 删除采用软删除，6 秒内可撤销并恢复到原位置；超过 6 秒后不提供恢复入口，30 天后物理清理，需要找回时依赖备份。

### 3.6 提醒与打扰控制

| 能力 | 规则 |
|---|---|
| 应用内提醒 | 到期由后端事件触发，窗口在托盘或最小化时先唤出主窗口再弹卡片；多条同一轮触发时合并为一张 |
| 应用内通知 | 主窗口可见时弹出，按钮「完成」「稍后 10 分钟」「打开」 |
| 任务栏 / 托盘 | 窗口未聚焦时请求注意；托盘 tooltip 显示最近一条待办标题与时间 |
| 稍后提醒 | 10 / 30 / 60 分钟。一次性条目：`snooze_until = max(now + minutes, remind_at)`，避免提前点铃铛把唯一一次提醒消耗掉；重复条目：`snooze_until = now + minutes`，因为它的 `remind_at` 已经指向下一次周期，取 `max` 会让「稍后」变成空操作。重复条目稍后到点只清空 `snooze_until`、不推进周期；若稍后发生在首次触发之前（条目已逾期），到点按正常规则推进一次 |
| 暂停提醒 | 顶部「⏸」或托盘菜单提供「1 小时 / 今天 / 取消暂停」；暂停期间不弹通知，结束后逾期条目合并提醒一次 |
| 免打扰 | 可设置 22:00–08:00；期间不弹通知、不标记已通知，结束后合并提醒一次；`quietStart > quietEnd` 视为跨零点 |
| 通知开关 | 关闭后只保留角标、列表、托盘提示，不弹应用内提醒，也**不写 `notified_at`**；重新打开后对仍到期且未通知的条目合并提醒一次 |

- 暂停和免打扰只影响「送达」，不改变条目的截止与完成状态。
- 暂停 / 免打扰结束后，如果有多条到期，只发一条聚合通知，避免补发一串。
- 状态在页面顶部常驻显示，例如「提醒已暂停至 18:30」「免打扰 22:00–08:00」。
- 稍后提醒**不改写截止时间**，只把真正触发的那一次顺延到 `snooze_until`：列表行显示 `已稍后 今天 18:40` 胶囊，抽屉提示改为「下次提醒：今天 18:40（已稍后）」，取 `max(due_at - remind_lead_min, snooze_until)`。到点触发后 `mark_fired` 清空 `snooze_until` 并广播 `notes:changed`，前端列表与抽屉同时刷新，避免残留「已稍后」。
- 快速记录的默认类型是 `todo`；如果左栏已经选中某个类型，快速记录沿用该类型。

### 3.7 截图转待办与图片附件

目标路径：一张聊天记录 / 会议通知 / 网页截图 → 自动整理成几条可编辑待办 → 原图跟着待办走，随时可回看。

**导入**

- 记事本顶部「截图生成」打开导入弹窗；支持 `Ctrl+V` 粘贴、拖拽、选择文件三种方式。
- 支持 `png` / `jpg` / `jpeg` / `webp`，单张 ≤ 8MB；超过 1600px 长边时前端先等比压缩一份用于识别，原图仍按原样保存为附件。
- 一张图一次识别；多张截图可重复导入，v1 不做多图合并。

**识别**

- 弹窗内明确显示「图片将发送至 {服务商} 进行识别」，由用户点击「开始识别」才发起请求，不在后台自动上传。
- 服务商取记事本设置里的「截图识别服务」，默认选中第一个标记为「支持图片」的服务商（`ai_providers.supports_vision = 1`）；一个都没有时按钮禁用，引导去「AI 助手 → 模型服务」添加并勾选「支持图片识别」。
- 识别期间显示进度与「取消」；超时 60 秒；失败时保留图片与当前结果，可重试或转手动录入。
- 返回结果只进入预览，不直接落库：左侧原图，右侧可编辑清单（标题、截止时间、重复、优先级、标签），每行带复选框，默认全选；低置信度或时间已过的行标黄提示。
- 顶部显示原文摘要与「识别到 N 条」；用户可删除误识别行、补充漏项、修改时间后统一「生成 N 条待办」。

**生成与关联**

- 确认后在一个事务里创建 N 条待办，并把同一张图片关联到每一条；任何一条失败则整批回滚，弹窗保留内容。
- 生成的待办默认落在「今天」或对应截止日期；列表跳转到包含它们的视图并提示「已生成 N 条，原图已附上」。
- 若没有识别出待办：提供「保存为一条记录」，标题取图片摘要或文件名，正文保留识别文字，图片照常挂上。

**附件查看**

- 编辑器右侧或底部显示缩略图条，单条最多 5 张；点击打开全屏预览，支持缩放、旋转、上一张 / 下一张、「另存为」。
- 列表行只显示 `📎 2` 这样的计数，不加载图片内容；打开编辑器或预览时才按需取图。
- 图片只读，不在应用内编辑或批注；删除附件只解除关联，仍被其他待办引用的文件不删除。

## 4. 数据模型

复用 `notes` 表。`kind` 用于区分待办 / 笔记 / 灵感 / 资料四种记录类型，但**不改变数据结构**：所有类型都可勾选完成，截止与提醒都是可选字段。`project_id`、`ref_type`、`ref_id` 仍保留不用，避免以后加字段迁移。

### 4.1 字段语义

| 字段 | 类型 | 语义 |
|---|---|---|
| `id` | INTEGER PK | 自增 |
| `title` | TEXT | 标题，≤ 200 字符；为空时取正文首行 |
| `content` | TEXT | 正文纯文本，≤ 200000 字符 |
| `kind` | TEXT | `todo` / `note` / `idea` / `reference`，默认 `todo`；写入时归一化，未知值回落 `todo`。可参与列表筛选 |
| `status` | TEXT | `open` / `done` / `deleted` |
| `priority` | INTEGER | 0 低 / 1 普通 / 2 高，仅用于列表标记与排序 |
| `tags` | TEXT | 逗号分隔，最多 20 个，单个 ≤ 32 字符；保存时去空、去重 |
| `due_at` | INTEGER | 截止时间，本地时区对应的 UTC 毫秒；NULL 表示无截止 |
| `remind_lead_min` | INTEGER | 提前量（分钟），0–10080；0 表示准点提醒 |
| `remind_at` | INTEGER | **下一次**提醒时间，保存时由 `due_at - remind_lead_min` 计算；重复触发后推进 |
| `remind_repeat` | TEXT | `none` / `daily` / `weekdays` / `weekly` / `monthly` |
| `snooze_until` | INTEGER | 稍后提醒的覆盖时间；非空时优先于 `remind_at` |
| `notified_at` | INTEGER | 最近一次已送达时间；一次性提醒据此去重 |
| `pinned` | INTEGER | 0 / 1，置顶排在最前 |
| `created_at` | INTEGER | 创建时间（UTC 毫秒） |
| `updated_at` | INTEGER | 最后修改时间（UTC 毫秒） |
| `done_at` | INTEGER | **新增**：最近一次完成时间；重复条目顺延时也会写入 |
| `deleted_at` | INTEGER | **新增**：软删除时间；30 天后清理 |
| `deleted_from_status` | TEXT | **新增**：删除前的 `status`（`open` / `done`）；恢复时据此还原，避免重复条目因 `done_at` 非空被误还原为已完成 |

`project_id`、`ref_type`、`ref_id` 暂不使用，保持 NULL / 默认值。图片附件不放在 `notes` 行内，单独用文件表 + 关联表，见 4.5。

### 4.2 状态与时间关系

```text
open ──(完成，一次性)──> done ──(取消完成)──> open
  │
  └──(完成，重复)──> open（due_at / remind_at 顺延，done_at 记录本次完成）

open / done ──(删除)──> deleted ──(撤销 / 30 天)──> open / 物理删除

提醒调度只看 status = open 的条目：
  有效触发时间 = snooze_until 非空 ? snooze_until : remind_at
```

- `due_at` 是用户看到的截止时间；`remind_at` 是调度器实际判断的时间，两者始终满足 `remind_at = due_at - lead`（重复顺延时同步平移，保持提前量）。
- 提醒必须依附截止时间。只填提醒时间、不填截止时间的设计不提供，避免两个时间概念互相打架。
- 已完成、已删除条目的 `remind_at` 保留用于回看，但不再参与调度。
- 删除时把原状态写进 `deleted_from_status`；恢复优先按它还原，不能只看 `done_at`——重复条目完成后仍是 `open`，但 `done_at` 已有值。

### 4.3 重复规则

以本地日历计算，避免固定 30 天造成「每月 31 日」漂移：

| 重复 | 推进规则 |
|---|---|
| `daily` | +1 天，保持时分 |
| `weekdays` | 下一个工作日（周一至周五）；周五完成后到周一 |
| `weekly` | +7 天，保持时分与星期 |
| `monthly` | +1 个月；目标月没有该日时取当月最后一天（1 月 31 日 → 2 月 28/29 日） |

- 推进时同时平移 `due_at` 与 `remind_at`，保持提前量不变。
- 循环推进直到 `remind_at > now`，用于应用长时间未启动后的补报（最多 1000 次，防御脏数据死循环）。该循环只在判定需要推进时执行；重复条目若已在首次触发时推进过 `remind_at`，稍后到点直接跳过推进；若 snooze 发生在首次触发前（条目已逾期），到点按正常规则推进一次，不额外跳期。
- 若 `due_at` 为空（理论不会出现，因为提醒依附截止时间），只推进 `remind_at`。
- 每月按「上月截止日」推进：1 月 31 日 → 2 月 28/29 日 → 之后以钳制后的日期为新锚点（3 月 28 日），不会自动回到 31 日；v1 接受这个取舍，不额外存原始日。
- 本地日历换算按 `chrono::LocalResult::earliest()` 取最早有效时刻；夏令时跳过的本地时刻顺延到下一个有效时刻，重复时刻取较早者。

### 4.4 设置

设置存 `app_meta` 的 `notes.settings`（JSON），默认值写在 Rust 代码里，不预置数据库行：

```ts
interface NotesSettings {
  notifyEnabled: boolean      // 默认 true，控制应用内提醒卡片
  quietHoursEnabled: boolean  // 默认 false
  quietStart: string          // 默认 "22:00"
  quietEnd: string            // 默认 "08:00"
  pauseUntil: number          // 默认 0，毫秒时间戳
  globalShortcut: boolean     // 默认 true
  visionProviderId: number | null // 默认 null；截图识别使用的 AI 服务商
}
```

- 读取时缺字段用代码默认值补齐，兼容后续加字段。
- `pauseUntil` 由「暂停 1 小时 / 暂停今天」写入（今天 = 次日 00:00）；到期自动失效，不需要额外清理任务。
- 免打扰判断按本地时间：`quietStart <= quietEnd` 时判断 `[start, end)`，`quietStart > quietEnd` 时判断 `>= start` 或 `< end`，天然覆盖跨零点。
- 全局快捷键固定 `Ctrl+Alt+N`；v1 不支持自定义组合，只支持开关。注册失败（被其他软件占用）时返回错误并把开关置为关闭。
- 启动时必须按已保存的设置注册一次：`lib.rs` 的 `setup` 里读取 `notes.settings`，`globalShortcut = true` 就注册，否则重启后快捷键会静默失效。注册与注销统一收敛在 `notes::quick::sync_shortcut(app, enabled)`，设置保存和启动都调它，避免两处逻辑漂移。
- `visionProviderId` 为空时，截图识别弹窗默认选中第一个 `supports_vision = 1` 的服务商（不落库），确认身份后写回；一个都没有时不自动选，按钮禁用并引导配置。列表来自 `ai_provider_list`，只做选择，不复制 Key。
- 为什么存 `app_meta` 而不是 `settings.json`：用户级 `AppSettings` 存 `settings.json`，但备份只对数据库做 `VACUUM INTO`，`settings.json` 只进诊断包；记事本设置放 `app_meta` 才能随备份一起恢复。代价是它不出现在设置页，读写只走 `notes_settings_get / notes_settings_set`。

### 4.5 图片附件表

图片按内容去重，数据库只存一份，多条待办通过关联表共享；备份 `VACUUM INTO` 会连 BLOB 一起带走。

```sql
CREATE TABLE IF NOT EXISTS note_files (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  sha256     TEXT NOT NULL,
  file_name  TEXT NOT NULL DEFAULT '',
  mime       TEXT NOT NULL,
  size       INTEGER NOT NULL,
  data       BLOB NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_note_files_sha ON note_files(sha256);

CREATE TABLE IF NOT EXISTS note_file_links (
  note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  file_id    INTEGER NOT NULL REFERENCES note_files(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (note_id, file_id)
);
CREATE INDEX IF NOT EXISTS idx_note_file_links_file ON note_file_links(file_id);
```

- 单张上限 8MB，只允许 `image/png`、`image/jpeg`、`image/webp`；入库前按魔数校验，不只信扩展名。
- 相同图片重复导入复用同一 `note_files` 行，只新增关联；单条待办最多 5 张图。
- 移除附件只删关联；待办软删除时保留关联，物理清理时由外键级联删除，再统一清理没有关联的 `note_files` 行，避免图片越积越多。
- 列表查询只 `COUNT(note_file_links)` 取附件数，不读 BLOB；详情 / 预览时按 `file_id` 单张取。
- SQLite 删除行不会缩小数据库文件（WAL + 未开 auto_vacuum），30 天清理只释放页到 freelist。孤儿清理后如果本次释放的图片总量超过 32MB，在 `heavy_ops` 锁内执行一次 `VACUUM`，把文件真正压回去；否则等下次备份前再由用户手动整理。不清空 `note_files` 的热数据时不要每轮都 VACUUM。

### 4.6 索引与迁移

```sql
-- schema.sql：notes 建表直接补列（新库建全）
--   kind TEXT NOT NULL DEFAULT 'todo'
--   done_at INTEGER
--   deleted_at INTEGER
--   deleted_from_status TEXT

-- schema.sql：索引不依赖新增列，每次启动幂等创建
CREATE INDEX IF NOT EXISTS idx_notes_status_due ON notes(status, due_at);

-- migrate()：依赖新增列，必须等补列之后再建
CREATE INDEX IF NOT EXISTS idx_notes_status_done ON notes(status, done_at DESC);

-- migrate() v5 → v6：归一化历史类型
UPDATE notes SET kind = 'todo' WHERE kind = 'note' AND due_at IS NOT NULL;
UPDATE notes SET kind = 'todo'
WHERE kind NOT IN ('todo', 'note', 'idea', 'reference');

-- schema.sql 建表 + COLUMN_MIGRATIONS 双写
--   ai_providers.supports_vision INTEGER NOT NULL DEFAULT 0
```

- `DB_VERSION` 从 4 升到 5（提醒与软删除新增 4 列），再从 5 升到 6（`notes.kind` 类型化）。老库按版本号顺序补列与归一化，重复启动幂等。
- 三个 `notes` 新列与 `ai_providers.supports_vision` 都按「`schema.sql` 建表 + `COLUMN_MIGRATIONS` 补列」双写，重复启动不报错。
- 不新建 `idx_notes_open_remind`：与既有 `idx_notes_due(status, remind_at)` 完全重复；也不建 `idx_notes_updated`，本地数据量用不上。
- 索引凡引用新增列，必须放 `migrate()` 事务内：`schema.sql` 先于补列执行，老库建索引会报 `no such column`。
- 两张附件表直接写进 `schema.sql`，每次启动 `CREATE TABLE IF NOT EXISTS`，老库自动补齐。
- 老库无论有没有 `notes` 表都由 `schema.sql` 建表 + 判存补列覆盖；v1.0.9 没有任何代码写入 `notes`，实际为空，无历史迁移成本。
- 启动时执行一次 `DELETE FROM notes WHERE status = 'deleted' AND deleted_at < now - 30 天`，并每天清理一次；随后删除无关联的 `note_files` 行。

## 5. 后端设计

### 5.1 模块划分

```text
src-tauri/src/
  notes/
    mod.rs        # 类型、CRUD、筛选、校验、重复时间计算、软删除清理
    quick.rs      # 迷你快速记录窗口的创建、显示、隐藏
    reminder.rs   # 调度循环、窗口唤出、托盘与提醒事件推送
    attachment.rs # 图片入库、内容去重、关联、取图、孤儿清理
    vision.rs     # 截图识别：服务商调用、JSON 校验、时间解析
  prompts/
    notes_extract.md # 截图识别提示词（include_str! 编译进二进制）
  commands/
    notes.rs      # IPC 命令薄封装
```

- `notes/mod.rs` 不依赖 Tauri 窗口，只依赖 `DbPool` 与 `chrono`，便于单测。
- `commands/notes.rs` 只做参数解析、调用领域层、返回错误字符串。
- 注册与依赖共六处：`lib.rs`（`mod notes`、命令列表、启动调度器、启动注册快捷键、托盘菜单）、`commands/mod.rs`（`pub mod notes`）、`Cargo.toml`（global-shortcut）、`capabilities/default.json`（`windows` 增加 `quick-capture`）、`.plugin(tauri_plugin_global_shortcut::Builder::new().build())`、`models.rs` + `commands/ai.rs` 的 `PROVIDER_COLUMNS` / INSERT / UPDATE 补 `supports_vision`。提醒不引入系统通知插件。
- 全局快捷键只在 Rust 侧注册，不引入 JS 包；前端通过设置命令开关。

### 5.2 IPC 契约

所有入参前端使用 camelCase，Rust 结构体 `#[serde(rename_all = "camelCase")]`。

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `notes_list` | `filter` | `NotesListResult` | 列表 + 各范围计数 |
| `notes_get` | `id` | `NoteDetail` | 单条完整正文 |
| `notes_create` | `input` | `NoteDetail` | 新建；触发 `notes:changed` |
| `notes_update` | `id`、`patch` | `NoteDetail` | 部分更新，未传字段不动 |
| `notes_toggle` | `id`、`done` | `ToggleResult` | 完成 / 取消完成；返回是否重复顺延 |
| `notes_snooze` | `id`、`minutes` | `NoteDetail` | 稍后提醒，1–1440 分钟 |
| `notes_delete` | `id` | — | 软删除，保留 30 天 |
| `notes_restore` | `id` | `NoteDetail` | 撤销删除 |
| `notes_clear_done` | — | `number` | 清空已完成：软删除（`status = 'deleted'`、`deleted_at = now`、`deleted_from_status = 'done'`），返回处理条数 |
| `notes_counts` | — | `NoteCounts` | 角标、工作台、托盘提示 |
| `notes_tags` | — | `TagCount[]` | 标签与数量 |
| `notes_settings_get` | — | `NotesSettings` | 读取设置 |
| `notes_settings_set` | `settings` | `NotesSettings` | 保存设置；处理快捷键注册 |
| `notes_quick_show` | — | — | 显示 / 聚焦迷你窗口（托盘与快捷键共用） |
| `notes_attachment_add` | `noteId`、`fileName`、`mime`、`base64` 或 `path` | `NoteFile` | 图片入库并按内容去重，建立关联；`path` 与 `base64` 二选一，来自文件选择器的图直接传路径，由 Rust 读盘，避免 8MB 走 JSON |
| `notes_attachment_list` | `noteId` | `NoteFile[]` | 只返回元数据，不含 BLOB |
| `notes_attachment_data` | `fileId` | `{ mime, base64 }` | 预览时按需取图 |
| `notes_attachment_save` | `fileId`、`path` | — | 「另存为」：由 Rust 直接写盘，不引入 `plugin-fs`，沿用 `capture_save_body` 的「前端选路径 + Rust 写文件」惯例 |
| `notes_attachment_remove` | `noteId`、`fileId` | — | 解除关联；无引用时删除文件 |
| `notes_extract_image` | `providerId`、`mime`、`base64`、`requestId` | `ExtractResult` | 只识别，不落库；当前时间由 Rust 取本地时钟，不由前端传；失败返回中文原因 |
| `notes_extract_cancel` | `requestId` | `boolean` | 取消进行中的识别请求 |
| `notes_create_from_image` | `fileName`、`mime`、`base64` 或 `path`、`tasks` | `CreateFromImageResult` | 事务内建 N 条待办 + 图片 + 关联 |

```ts
interface NotesFilter {
  scope: 'today' | 'week' | 'all' | 'done'
  keyword?: string
  tag?: string
  kind?: 'todo' | 'note' | 'idea' | 'reference'   // 缺省 = 不按类型过滤
  limit?: number       // 默认 200，最大 500
}

interface NotesListResult {
  items: NoteSummary[]
  counts: NoteCounts
}

interface NoteCounts {
  open: number
  dueToday: number     // 今天到期且未完成，不含逾期
  overdue: number      // 已过截止时间且未完成
  done: number
}

interface NoteSummary {
  id: number
  title: string
  excerpt: string      // 正文前 120 字
  status: 'open' | 'done' | 'deleted'
  priority: number
  tags: string[]
  dueAt: number | null
  remindAt: number | null
  remindLeadMin: number
  repeat: 'none' | 'daily' | 'weekdays' | 'weekly' | 'monthly'
  snoozeUntil: number | null
  pinned: boolean
  doneAt: number | null
  updatedAt: number
  attachmentCount: number   // 列表只取数量，不读 BLOB
}

interface NoteDetail extends NoteSummary {
  content: string
  createdAt: number
}

interface ToggleResult {
  note: NoteDetail
  rolledTo: number | null   // 重复条目顺延后的下次截止时间
}

interface TagCount {
  name: string
  count: number
}

interface NoteFile {
  id: number
  fileName: string
  mime: string
  size: number
  createdAt: number
}

interface ExtractItem {
  title: string
  detail: string
  dueAt: number | null       // 后端把模型返回的本地时间换算成 UTC 毫秒
  repeat: 'none' | 'daily' | 'weekdays' | 'weekly' | 'monthly'
  priority: number           // 0 / 1 / 2
  tags: string[]
  warning: string | null     // 时间已过、只识别到部分内容等提示
}

interface ExtractResult {
  items: ExtractItem[]
  summary: string            // 图片内容摘要，最多 200 字
  rawText: string            // 识别出的原文，便于「保存为一条记录」
}

interface CreateFromImageResult {
  noteIds: number[]
  fileId: number
}
```

`NoteInput` / `NotePatch` 的字段与上表一致；`dueAt`、`priority`、`tags`、`pinned` 可选。`dueAt` 需要区分「未传」与「显式清空」，实现采用 `Option<Option<i64>>` + 自定义 `double_option` 反序列化：字段缺省 = 不动，`null` = 清空，数值 = 设置。清空截止会联动清空提前量、重复规则、`remind_at`、`snooze_until` 与 `notified_at`，编辑器保存前需二次确认。

### 5.3 校验规则

- 标题 trim 后为空：取正文首行前 200 字；正文也为空 → `E_NOTES_EMPTY`「标题和正文不能同时为空」。
- 正文超过 200000 字符 → `E_NOTES_TOO_LONG`。
- `due_at` 只接受 2000-01-01 到 2100-01-01 之间的毫秒值。
- 新建或更新时只要 `due_at`、`remind_lead_min` 或 `remind_repeat` 发生变化，就重算 `remind_at = due_at - lead`，并**同时清空 `notified_at` 与 `snooze_until`**：否则一次性条目改期后仍带着旧的已通知标记，调度查询永远不再命中，表现为「改了时间再也不提醒」。
- `due_at` 被清空时强制清空 `remind_at`、`snooze_until`、`notified_at`，并把 `remind_lead_min` 归零、`remind_repeat` 置为 `none`（属于破坏性联动，编辑器需二次确认）。
- `remind_lead_min` 限制 0–10080；超出按边界截断，不报错。
- `repeat`、`priority` 使用白名单，非法值回退默认值（`none` / 1）。
- `snooze` 分钟限制 1–1440；只接受 `status = 'open'`、`due_at` 与 `remind_at` 均非空的条目。一次性条目写 `snooze_until = max(now + minutes, remind_at)`；重复条目写 `snooze_until = now + minutes`（它的 `remind_at` 已指向下一周期，再取 `max` 会让稍后失效）。两者都置 `notified_at = NULL`。
- 标签按中英文逗号拆分，去空、去重、截断到 20 个。
- 设置里的免打扰时间必须是 `HH:mm`；非法值回退默认。
- 图片只接受 `png` / `jpeg` / `webp`，解码后 ≤ 8MB，并按文件头魔数校验真实类型；非图片或超限返回中文错误。
- 单条待办关联图片 ≤ 5 张；`notes_create_from_image` 的 `tasks` 数量限制 1–50，标题沿用普通校验。
- 识别结果里的时间必须在 2000-01-01 到 2100-01-01 之间，超出按 `null` 处理并加警告；`repeat` 不在枚举内回退 `none`。

### 5.4 筛选、排序与分组

| scope | SQL 条件（本地时区换算后） |
|---|---|
| `today` | `status = 'open' AND (due_at IS NULL OR due_at < 明天 00:00)`（含全部逾期与无截止） |
| `week` | `status = 'open' AND (due_at IS NULL OR due_at < 下周一 00:00)`（含全部逾期与无截止） |
| `all` | `status = 'open'` |
| `done` | `status = 'done'`，按 `done_at DESC` |

- 所有列表默认排除 `status = 'deleted'`。
- 置顶始终最前；其余按「逾期 → 今天 → 之后 → 无截止」分组，组内按优先级、截止时间、更新时间排序。
- 前端接收已排序的列表，按 `dueAt` 与本地日期分组渲染吸顶标题；分组逻辑抽成 `notes/grouping.ts` 便于测试。
- `keyword` 同时匹配标题与正文，转义 `%`、`_`、`\`，使用 `LIKE ... ESCAPE '\'`。
- `tag` 按逗号边界匹配，避免 `#工作` 命中 `#工作台`。
- `notes_tags` 统计未删除条目的标签使用次数，供左栏筛选与编辑器联想；SQLite 没有字符串拆分函数，按 Rust 读出 `tags` 列聚合，不为标签单独建表。
- `all` 视图超过 500 条时只返回前 500 条，前端在列表底部提示「仅显示前 500 条，请用搜索或筛选缩小范围」，不静默截断。

### 5.5 提醒调度器

启动方式：`lib.rs` 的 `setup` 中调用 `notes::reminder::spawn(app.handle().clone())`。

```text
启动后 8 秒首次执行（等前端页面挂载完）
之后每 30 秒执行一次
每次：
  1. 读取设置：
       pauseUntil > now            → 跳过本轮，不标记
       当前处于免打扰时段          → 跳过本轮，不标记
       notifyEnabled = false       → 跳过本轮，不标记（与暂停一致）；重新打开后对仍到期且未通知的条目合并提醒一次
  2. 用 spawn_blocking 查到期条目（LIMIT 200）
  3. 在事务里逐条处理：
       notified_at = now
       若 remind_at > now（稍后提醒场景，下次提醒已排好）
         → 只清空 snooze_until，remind_at / due_at 保持不动
       否则
         → 清空 snooze_until
         → 若 repeat != none：remind_at / due_at 顺延到严格晚于 now 的下一个周期
  4. 提交后：
       有到期条目 → 窗口隐藏或最小化时先 show + unminimize（开关打开时）
       发 notes:reminder 事件，由前端弹一张聚合提醒卡片
       更新托盘 tooltip 为最近一条待办
```

到期查询：

```sql
SELECT id, title, due_at, remind_at, remind_repeat
FROM notes
WHERE status = 'open'
  AND remind_at IS NOT NULL
  AND (
    (snooze_until IS NOT NULL AND snooze_until <= ?now)
    OR
    (snooze_until IS NULL AND remind_at <= ?now)
  )
  AND (COALESCE(remind_repeat, 'none') <> 'none' OR notified_at IS NULL)
ORDER BY COALESCE(snooze_until, remind_at) ASC
LIMIT 200
```

- 单轮上限 200 而不是 50：暂停 / 免打扰结束后要「只补一条聚合通知」，如果积压超过一轮上限，就会连续几轮各弹一条，违背承诺。200 对本地记事本的数据量足够；真超过 200 时下一轮继续，仍按聚合发送。

- 一次性提醒：`notified_at` 非空即不再触发。
- 重复提醒：`notified_at` 不参与去重，靠 `remind_at` 推进到未来避免重复；`snooze_until` 优先。
- 暂停 / 免打扰 / 通知开关关闭期间直接跳过查询，不写库；结束后一次性补报，避免补发一串历史通知。
- 「先标记、后通知」：极端情况下进程在两步之间崩溃会少一条通知，但条目仍会以逾期状态出现在列表，不会无限重复打扰。
- 调度器单循环运行，命令侧更新带 `WHERE id = ? AND status = 'open'` 条件，避免与完成、删除并发时写回已结束的条目。

### 5.6 完成、稍后与撤销

**一次性条目完成**：`status = 'done'`、`done_at = now`、`notified_at = now`、`snooze_until = NULL`。

**重复条目完成**：

1. 记录 `done_at = now`。
2. 判断下一次是否已排好，只认 `remind_at > now`：说明本次提醒已触发、调度器已把周期推到下一次，`due_at / remind_at` 保持不动；否则从当前 `due_at / remind_at` 先推进一个周期，仍不晚于当前时间时继续推进，直到晚于当前时间。这样提前完成只顺延一次，不会因为「永远追到未来」而跳过下一次。**不能把 `snooze_until` 非空当作「已触发」**：逾期条目在首次 tick 前点稍后也会有 `snooze_until`，但 `remind_at` 还在过去，按旧判据会保持不动，完成后立刻又显示逾期。
3. `status` 保持 `open`，`notified_at = NULL`，`snooze_until = NULL`。
4. 返回 `rolledTo`，前端提示「已顺延到 X」。
5. 若要终止重复：编辑器把「重复」改为「不重复」后再完成。

**取消完成**：`status = 'open'`、`done_at = NULL`。一次性条目保留原 `notified_at`，不会立刻重复弹窗；需要再提醒时用「稍后提醒」或重新设置截止时间。

**稍后提醒**：仅接受 `status = 'open'` 且 `due_at / remind_at` 非空的条目，`notified_at = NULL`、`updated_at = now`。一次性条目 `snooze_until = max(now + minutes, remind_at)`，防止提前点铃铛把唯一一次提醒消耗掉；重复条目 `snooze_until = now + minutes`，因为它的 `remind_at` 已经是下一次周期，取 `max` 会让「稍后」无声失效。UI 只在已到期或已进入提前量窗口时显示按钮。重复条目若已在首次触发时推进过 `remind_at`，稍后到点只清空 `snooze_until`；若 snooze 发生在首次触发前（条目已逾期），到点按正常规则推进一次，不额外跳期。

**删除与恢复**：删除写 `status = 'deleted'`、`deleted_at = now`、`deleted_from_status = 原 status`，保留 `done_at`；恢复时按 `deleted_from_status` 还原为 `open` 或 `done`，并清空 `deleted_at`、`deleted_from_status`。不能只看 `done_at`：重复条目完成后 `status` 仍是 `open`，但 `done_at` 已有值。恢复后按原时间重新参与调度，30 天后物理清理。

**清空已完成**：与普通删除同一套软删除，逐条写 `status = 'deleted'`、`deleted_at = now`、`deleted_from_status = 'done'`；30 天内仍可依赖备份找回，期间关联图片不删除，物理清理后才触发孤儿图片回收。

### 5.7 通知与托盘

- 提醒只走应用内卡片（Ant Design Vue `notification`），不再调用系统通知，避免开发模式下出现 PowerShell 署名与双份打扰。
- 卡片标题为第一条待办标题，描述为「MM-DD HH:mm · 已逾期」；多条时标题为「N 条待办到期」，正文列出前 3 条。
- 卡片常驻（`duration: 0`），提供「完成 / 稍后 10 分钟 / 打开」；多条聚合时提供「打开记事本 / 全部稍后 10 分钟」。
- 窗口在托盘或最小化时，调度器先 `show + unminimize` 再发事件，保证卡片有可见容器；同时调用 `request_user_attention`（窗口未聚焦时）并更新托盘 tooltip。
- 托盘 tooltip 优先显示「HH:mm 标题」，无待办时恢复为 `AstroMan`；暂停期间显示「AstroMan · 提醒已暂停」。更新用 `app.tray_by_id("main-tray")`，现有 `lib.rs` 建完未持有句柄。
- 托盘菜单新增：快速记录、暂停提醒（1 小时 / 今天）、打开记事本。菜单事件调用同一套领域函数。
- 只在窗口隐藏或最小化时唤出，不主动 `set_focus`，避免抢走当前输入焦点；窗口已可见时只弹卡片。

### 5.8 事件

```ts
// notes:reminder
interface NotesReminderEvent {
  items: Array<{
    id: number
    title: string
    dueAt: number | null
    overdue: boolean
  }>
  firedAt: number
}

// notes:changed：任意新建 / 更新 / 完成 / 删除 / 恢复 / 设置变化后发出
interface NotesChangedEvent {
  reason: 'create' | 'update' | 'toggle' | 'delete' | 'restore' | 'settings'
  id?: number
}
```

- 主窗口收到 `notes:reminder` 后刷新计数与列表，窗口可见时弹应用内通知。
- 调度器 `mark_fired` 之后额外发一次 `notes:changed`（`reason = 'update'`），让列表与抽屉把已经失效的「已稍后」标记刷新掉。
- 主窗口与迷你窗口都监听 `notes:changed`；迷你窗口保存后主窗口无需手动刷新。
- 事件失败不影响写库结果，前端下次进入页面或 60 秒轮询会校正。

### 5.9 图片附件

- 入库：来自「选择文件」的图片直接传绝对路径，由 Rust 读盘；来自粘贴 / 拖拽的图片没有路径，才回退 base64。后端拿到字节后按魔数判定真实类型，计算 SHA-256，先查 `note_files.sha256`，命中则复用，未命中才写 BLOB。两条入口共用同一个解码函数，校验规则一致。
- `notes_create_from_image` 在一个事务里完成：建 N 条 `notes` → 复用或写入一张 `note_files` → 插入 N 条 `note_file_links` → 提交后发一次 `notes:changed`（`reason = 'create'`）。任一步失败全部回滚，弹窗保留用户编辑。
- `notes_attachment_data` 只允许取「至少有一条关联」的文件，避免按自增 id 枚举读图；返回 base64 与 mime，前端转 `data:` URL 给预览组件。
- `notes_attachment_remove` 删关联；关联数归零时再删 `note_files` 行。物理清理（30 天软删除）后同样执行一次孤儿清理。
- 列表查询用 `LEFT JOIN (SELECT note_id, COUNT(*) c FROM note_file_links GROUP BY note_id)` 得到 `attachmentCount`，不触碰 BLOB。
- 图片只读，不生成缩略图副本：列表不加载图，编辑器缩略图直接用原图按 CSS 缩放；打开预览时才取完整数据。
- 传输代价：只有粘贴 / 拖拽的入库和 `notes_attachment_data` 走 base64，8MB 原图约 11MB JSON，单张按需加载；如预览明显卡顿，改用 `tauri::ipc::Response` 直接返回二进制，前端转 Blob URL。
- 「另存为」走 `notes_attachment_save(fileId, path)`，路径由前端 `plugin-dialog.save()` 选好后传给 Rust 写盘；项目没有 `plugin-fs`，也不为此新增依赖。
- 空间回收：软删除 30 天清理只删行，SQLite 文件不会自动变小；当本轮孤儿清理释放的图片总量超过 32MB 时，在 `heavy_ops` 锁内补一次 `VACUUM`（见 4.5），否则库文件会长期停在高水位。

### 5.10 截图识别

**调用链路**

- 服务商取 `visionProviderId`（参数优先，空则读设置）；只接受 `supports_vision = 1` 的服务商，未标记时直接返回「当前服务商未标记为支持图片识别」；复用 `ai_providers` 的解密 Key 逻辑，不新增密钥存储。
- 按 `kind` 组装多模态请求，`stream = false`、`temperature` 沿用服务商配置（未配置时不传，避免 o 系列 / thinking 模型直接 400）、`max_tokens = 4096`（20 条 JSON 大约 2400+ token，2048 会被截断）、超时 60 秒：

```text
openai     messages: [{ role: "user", content: [
             { type: "text", text: prompt },
             { type: "image_url", image_url: { url: "data:{mime};base64,{data}" } }
           ]}]
anthropic  content: [
             { type: "image", source: { type: "base64", media_type, data } },
             { type: "text", text: prompt }
           ]
gemini     parts: [
             { inline_data: { mime_type, data } },
             { text: prompt }
           ]
```

- 前端传入的图片已按长边 1600px、JPEG 质量 0.85 压缩，减少 token 与超时；原图另走附件入库，不受压缩影响。
- 请求 ID 存进 `AppState` 的取消表（沿用 `ai_chat_cancel` 的做法）；`notes_extract_cancel` 触发 `CancellationToken`，弹窗关闭即取消。
- 响应只取文本；用量写入现有 `ai_usage_logs`，`request_preview` 统一加 `[截图识别]` 前缀加图片摘要，和航仔聊天记录区分开，不记录 base64。
- 当前本地时间、星期、时区偏移由 Rust 用 `chrono::Local` 在发请求时生成，并替换提示词里的占位符；不接受前端传入的 `nowMs`，避免客户端时钟与后端不一致。

**提示词约束**（`prompts/notes_extract.md`）

- 注入当前本地时间、星期与时区偏移，要求模型按本地时间理解「明天」「周五」「下月 3 号」。
- 只提取明确或强烈暗示的可执行事项；不臆造、不补全缺失信息；把同一事项的多句话合并成一条。
- 输出严格 JSON，不要 Markdown 代码块：

```json
{
  "summary": "图片内容一句话摘要",
  "raw_text": "识别到的关键原文",
  "items": [
    {
      "title": "提交周报",
      "detail": "周五下班前发到群里",
      "due_at": "2026-11-06 18:00",
      "repeat": "weekly",
      "priority": 1,
      "tags": ["工作"],
      "warning": null
    }
  ]
}
```

- 没有明确时间时 `due_at` 为 `null`；只有重复词没有日期时，由后端按 4.3 规则取下一次未来时刻；明确写了已过去的时间时保留原时间并加 `warning: "时间已过"`。
- 标题 2–60 字；最多 20 条；`repeat` 只允许五个枚举值；`priority` 只在明确出现「紧急 / 重要 / 尽快」时给 2。
- 模型返回空数组是合法结果，前端转为「保存为一条记录」入口。

**解析与兜底**

- 去掉 ``` 围栏，截取第一个 `{` 到最后一个 `}`，再按 JSON 解析；字段名同时兼容 `dueAt` / `due_at`、`rawText` / `raw_text`。
- 首次解析失败：用同一张图重试一次，追加「只返回 JSON，不要解释」；仍失败返回 `E_NOTES_EXTRACT_FORMAT`，前端提示手动录入，图片保留。
- 时间字段接受 `YYYY-MM-DD HH:mm`、`YYYY-MM-DD`、ISO 8601；按本地时区换算成 UTC 毫秒；无法解析或越界置 `null` 并加警告。
- 服务商错误映射：401 → 「API Key 无效」；404 / model → 「模型不存在或当前账号无权使用」；包含 image / vision / multimodal → 「当前模型不支持图片输入，请换用视觉模型」；超时 → 「识别超时，可重试或直接手动创建」。
- 日志只记服务商、模型、耗时、状态码与错误摘要，绝不打印 base64。

## 6. 前端设计

### 6.1 新增与修改文件

| 文件 | 动作 | 职责 |
|---|---|---|
| `src/types/notes.ts` | 新增 | 与 Rust 对齐的类型 |
| `src/api/notes.ts` | 新增 | 22 个 IPC 封装 |
| `src/stores/notes.ts` | 新增 | 计数、设置、提醒状态、节流刷新 |
| `src/utils/notesQuickParse.ts` | 新增 | 自然语言速记解析 |
| `src/utils/notesGrouping.ts` | 新增 | 日期分组、截止时间格式化 |
| `src/utils/notesImage.ts` | 新增 | 图片读取、类型校验、长边压缩、data URL 转换 |
| `src/utils/notesTags.ts` | 新增 | 标签名哈希到 8 组固定色调，保证同一标签各处颜色一致 |
| `src/views/Notes.vue` | 新增 | 两栏页面（筛选 + 列表）、类型筛选、快速记录、截图入口、50% 详情抽屉 |
| `src/components/notes/NotesEditor.vue` | 新增 | 抽屉内编辑表单（类型 / 标签 / 附件 / 截止提醒）、附件条与自动保存 |
| `src/components/NotesImportDialog.vue` | 新增 | 截图导入、识别进度、可编辑待办清单 |
| `src/components/NotesImageViewer.vue` | 新增 | 全屏预览：缩放、旋转、切换、另存为 |
| `src/components/NotesReminderHost.vue` | 新增 | 监听事件、弹出常驻应用内提醒卡片（无系统通知、无权限申请） |
| `src/views/QuickCapture.vue` | 新增 | 迷你快速记录窗口内容 |
| `src/router/index.ts` | 修改 | 增加 `/notes` 与 `/quick-capture`（`meta.bare = true`） |
| `src/App.vue` | 修改 | 导航项、角标、提醒宿主、裸布局、60 秒计数刷新；裸布局必须同时跳过全局快捷键（Ctrl+K / Ctrl+1..9 / Alt+←）、AstroBuddy 悬浮球与 60 秒轮询，否则在迷你窗按 Ctrl+1 会把迷你窗导航到工作台 |
| `src/views/Dashboard.vue` | 修改 | 「今日待办」卡片与快速记录 |
| `src/views/AiAssistant.vue` | 修改 | 服务商表单增加「支持图片识别」勾选，写入 `ai_providers.supports_vision`；列表可显示该标记 |
| `src/types/models.ts` | 修改 | `AiProvider` 增加 `supportsVision`；不改这里前端拿不到标记 |
| `package.json` | 修改 | 不新增通知插件（提醒走 Ant Design Vue `notification`）；也不新增 `plugin-fs`（另存为走 Rust 命令） |

### 6.2 页面状态

```ts
const scope = ref<'today' | 'week' | 'all' | 'done'>('today')
const keyword = ref('')
const tag = ref('')
const kind = ref<NoteKind | ''>('') // 空 = 全部类型
const selectedId = ref<number | null>(null) // 非空时打开右侧抽屉
const items = ref<NoteSummary[]>([])
const detail = ref<NoteDetail | null>(null)
const counts = ref<NoteCounts>({ open: 0, dueToday: 0, overdue: 0, done: 0 })
const settings = ref<NotesSettings>(defaultNotesSettings())
const importOpen = ref(false)
const importImage = ref<{ base64: string; mime: string; fileName: string; previewUrl: string } | null>(null)
const extractState = ref<'idle' | 'loading' | 'ready' | 'error'>('idle')
const extractItems = ref<ExtractItem[]>([])
const viewMode = ref<'list' | 'card'>('list') // 持久化到 astroman:notes:view-mode
const customTags = ref<string[]>([]) // 持久化到 astroman:notes:custom-tags
const tagInputOpen = ref(false)
const tagInput = ref('')
```

- 切换筛选 / 搜索 / 类型：请求 `notes_list`，保持当前选中项（若不在结果中则清空抽屉）。
- 点击条目：请求 `notes_get` 加载完整正文并打开右侧抽屉；未保存的编辑先 flush。抽屉宽度 = 窗口 50%（`Math.max(520, width * 0.5)`），窄窗口下取窗口减 32px、上限 560px。
- 新建：顶部「+ 新建▾」选类型后调 `notes_create` 并直接打开抽屉；若当前类型筛选与新记录不一致，自动切到新记录所属类型，避免「建完就看不见」。
- 快速记录：`notes_create` 成功后清空输入、刷新列表与计数，保持输入框焦点。
- 完成勾选：乐观更新列表，调用 `notes_toggle`；失败回滚并提示。一次性完成弹 6 秒撤销，重复顺延提示新日期。
- 删除：软删除后乐观移除；6 秒内撤销调用 `notes_restore`。
- 暂停 / 免打扰：`notes_settings_set` 保存，顶部状态条与通知宿主同步。
- 视图切换：列表 / 卡片只影响渲染，不重新请求；标签侧栏显示 `notes_tags` 与本地新建的空标签的并集，新建标签只写本机 `localStorage`，不落库建表。

### 6.3 编辑器与自动保存

- 字段：类型（待办 / 笔记 / 灵感 / 资料）、标题、正文、截止时间（精确到分钟）、提前量（0 / 5 / 15 / 30 / 60 / 1 天 / 自定义）、重复（不重复 / 每天 / 工作日 / 每周 / 每月）、优先级、标签、置顶。
- 截止时间为空时，提前量与重复置灰并清空；点击清空截止时间需二次确认（会同时丢失提前量与重复规则），这是「提醒依附截止时间」的直接体现。
- 编辑区实时显示「下次提醒：今天 17:45（15 分钟后）」；截止时间已过时提示「保存后会立即提醒」；已点过「稍后」时显示「下次提醒：今天 18:40（已稍后）」，取 `max(due - lead, snoozeUntil)`。
- 修改后 600ms 防抖调用 `notes_update`，状态显示「保存中 / 已保存 / 未保存」。
- 切换条目前强制 flush；保存失败保留本地内容并提示，不关闭编辑器。
- 标签输入支持从 `notes_tags` 联想，回车添加，退格删除。
- 快捷键：`Ctrl/Cmd + S` 立即保存；`Esc` 收起编辑器并 flush。
- 编辑器支持 `Ctrl+V` 粘贴图片与拖拽图片：读取 → 校验 → 调 `notes_attachment_add`，成功后刷新附件条；失败提示原因，不影响正文保存。
- 附件条显示缩略图与 `📎 n/5`；悬停可删除单张，点击打开全屏预览。超过 5 张时按钮禁用并提示。

### 6.4 迷你快速记录窗口

- 路由 `/quick-capture`，`meta.bare = true`，`App.vue` 只渲染路由内容，不带侧边栏与页头。
- 窗口由 Rust 创建并复用：首次 `notes_quick_show` 构建，之后只 `show + set_focus`；关闭请求改为隐藏。
- 窗口参数：520×200、无边框、置顶、跳过任务栏、居中、不可缩放；使用系统窗口阴影。
- 内容：输入框自动聚焦、解析预览标签、底部提示「Enter 保存 · Esc 关闭 · Ctrl+Alt+N 随时唤起」。
- 保存：`notes_create` → 清空草稿 → `hide()`；失败保留输入并显示错误。
- 草稿：输入即写 `localStorage`（`astroman:notes:quick-draft`），保存或手动清空后删除。
- 中文输入法：回车保存需判断 `compositionstart/end`，输入法候选期间不提交。
- `tauri_plugin_window_state` 用 `.with_denylist(&["quick-capture"])` 排除迷你窗口，避免记忆一个固定位置后挡住用户屏幕。
- `capabilities/default.json` 的 `windows` 数组必须包含 `quick-capture`（权限沿用 `core:default`），否则该页面里的 `invoke` 会被拒绝。
- 全局快捷键回调若不在主线程，需 `app.run_on_main_thread` 再创建/显示窗口；重复唤起只 `show + set_focus`，不重复构建。
- `/quick-capture` 不写入导航历史、也不写 `astroman:last-route`：两个窗口共用同一份 `localStorage`，否则下次启动主窗口会还原成裸页面。
- 快捷键注册失败（被其他应用占用）时静默降级，仅在设置区提示，不影响主流程。

### 6.5 工作台卡片与角标

- 工作台在「快捷入口」上方插入「今日待办」卡片：
  - 左侧：逾期 / 今天到期数量，前 5 条（标题、截止时间、复选框）。
  - 右侧：快速记录输入框 + 「打开记事本」。
- 卡片数据来自 `notes_counts` + `notes_list({ scope: 'today' })`；页面挂载时请求一次，收到 `notes:changed` 或窗口重新可见时刷新。
- 侧边栏角标取 `overdue + dueToday`，`App.vue` 每 60 秒刷新一次，并在窗口重新可见时刷新。
- 角标不显示「全部未完成」数量，避免数字长期很大失去提示意义。

### 6.6 应用内提醒

- 使用 Ant Design Vue 的 `notification`，`duration: 0`（常驻，直到用户操作或关闭），卡片带主题色左边条与淡主色渐变。
- 单条：标题为记录标题，描述为截止时间 / 逾期时长；按钮「完成」「稍后 10 分钟」「打开」。
- 多条：标题「N 条待办到期」，描述列出前 3 条；按钮「打开记事本」「全部稍后 10 分钟」。
- 仅在 `notifyEnabled = true` 时弹出；窗口隐藏或最小化时由 Rust 侧先唤出主窗口，保证卡片可见。
- `NotesReminderHost` 只在主布局挂载；迷你窗口是裸布局，不重复弹通知，只负责录入。
- 通知里的操作直接调用 API，成功后关闭通知并刷新列表；失败保留通知并提示。

### 6.7 提醒开关

- 不再申请系统通知权限，也不再发送 Windows / macOS 系统通知。
- 设置里的「到点提醒」开关只控制应用内卡片；关闭后角标、列表与托盘 tooltip 仍正常更新。

### 6.8 截图导入与图片预览

**导入弹窗**

- 顶部工具栏「截图生成」打开 `NotesImportDialog`，宽 900px；左侧图片区、右侧清单区，窄屏改为上下布局。
- 图片区初始是虚线放置区：「粘贴截图（Ctrl+V）· 拖入图片 · 选择文件」；粘贴事件从 `clipboardData.items` 取 `image/*`，拿不到时提示用「选择文件」。
- 选择或拖入后立即展示预览与文件名、尺寸；超过 8MB 直接拒绝，超过 1600px 只压缩识别副本，附件仍存原图。
- 未选择识别服务商、或没有任何 `supports_vision = 1` 的服务商时，右侧显示引导与「去配置模型服务」链接（跳到 AI 助手页），不发起请求。
- 「开始识别」按钮旁常驻一行灰字：「图片将发送至 {服务商名}，仅用于本次识别」；点击即视为同意发送。

**识别结果编辑**

- 清单每行：复选框、标题输入、截止时间选择器（精确到分钟）、重复下拉、优先级、标签、删除按钮；不可识别的时间显示为空，由用户补。
- 行内 `warning` 用浅黄底提示，例如「时间已过」「未写明年份，已按下一次推断」。
- 顶部：`识别到 N 条 · 全选 / 取消全选`，以及「重新识别」；底部：`取消`、`生成 N 条待办`（N 为勾选数）。
- 生成中按钮 loading、弹窗不可关闭；成功后关闭并刷新列表、计数，定位到新条目所在视图。
- 识别失败：保留图片与已填内容，展示错误与「重试」；「转为手动记录」直接切到空白清单。
- 空结果：清单区显示「没有识别出待办」+「保存为一条记录」；保存时标题取 `summary` 或文件名，正文取 `rawText`。

**图片预览**

- `NotesImageViewer` 用 Ant Design Vue 的 `Image` 预览组实现基础缩放 / 旋转 / 左右切换，额外提供「另存为」按钮（`save` 对话框 + base64 写文件）。
- 预览数据按 `fileId` 懒加载，打开时显示骨架与加载失败重试；不预加载全部附件。
- 关闭预览立即释放 `objectURL` / base64 引用，避免多张大图长期占内存。

## 7. 边界与错误处理

| 场景 | 处理 |
|---|---|
| 应用隐藏到托盘 / 最小化 | 调度器继续运行；到点时 Rust 先 `show` + `unminimize` 主窗口，再发事件弹出应用内提醒卡片，保证卡片有可见容器 |
| 托盘「退出」或进程被杀 | 不再提醒；下次启动 8 秒内补报所有逾期未通知条目 |
| 电脑睡眠 / 休眠唤醒 | 下一次 tick 立即补报，最多 50 条/轮 |
| 系统时间被向前调 | 已通知的一次性条目不重复；重复条目按新时间重新推进 |
| 系统时间被向后调 | 已过期条目可能再次进入到期窗口，最多再提醒一次 |
| 新建时截止时间已经过去 | 下一轮 tick 视为到期立即提醒一次，不会因为「刚创建」而跳过 |
| 已提醒过的条目改截止时间 / 提前量 / 重复 | 重算 `remind_at` 并清空 `notified_at`、`snooze_until`，按新时间重新提醒；否则一次性提醒会被旧标记永久压住 |
| 重复条目刚触发后在通知里点「稍后」 | 写入 `snooze_until = now + minutes`，到点只补一次提醒，`remind_at` 指向的下个周期保持不变 |
| 快速记录不带日期 | 落在「今天 / 本周 / 全部」末尾的「无截止」组，不会保存后从默认视图消失 |
| 迷你窗口里按 Ctrl+1 / Ctrl+K | 裸布局跳过全局快捷键与悬浮球，导航类快捷键不生效 |
| 从文件选择器导入 8MB 截图 | 直接传路径给 Rust 读盘，避免 11MB JSON；粘贴 / 拖拽没有路径时才走 base64 |
| 30 天清理后数据库没变小 | SQLite 只把页放回 freelist；本轮释放超过 32MB 时在 `heavy_ops` 锁内 `VACUUM` |
| 暂停 / 免打扰 / 到点提醒开关关闭期间到期 | 不弹卡片、不标记；结束后合并提醒一次 |
| 提前对未到期条目点「稍后」 | 按钮禁用；后端仍以 `max(now + minutes, remind_at)` 兜底，不会提前消耗一次性提醒 |
| 重复条目完成一次后删除再恢复 | 按 `deleted_from_status` 还原为 `open`，不会因为 `done_at` 非空变成已完成 |
| 清空已完成 | 软删除，30 天后物理清理；期间列表计数排除，图片仍被关联保留 |
| 提醒卡片已存在时又到期 | Ant Design Vue `notification` 按 `key` 更新同一条卡片，不叠加刷屏；多条到期合并成一条聚合卡片 |
| 全局快捷键被占用 | 注册失败返回错误并关闭开关，设置页提示「快捷键被占用，可手动打开记事本」 |
| 迷你窗口输入法组合中按回车 | 不提交，等候选词确认后再判断 |
| 迷你窗口保存时主窗口隐藏 | 主窗口通过事件刷新角标；下次打开即可看到 |
| 数据库写失败 | 命令返回中文错误，前端保留编辑内容并提示重试 |
| 条目在编辑时被删除 | `notes_update` 返回「记录不存在」，前端关闭编辑器并刷新列表 |
| 正文过长 | 后端拒绝并提示 20 万字符上限 |
| 未配置视觉服务商 | 截图弹窗禁用「开始识别」，引导到「AI 助手 → 模型服务」；仍可保存图片、手动建待办 |
| 当前模型不支持图片 | 根据服务商错误提示「请换用视觉模型」，保留图片与编辑内容 |
| 识别超时 / 网络失败 | 60 秒超时，保留图片与结果，可重试；取消后丢弃结果 |
| 模型返回非 JSON | 自动重试一次；仍失败提示手动录入，并保留 `rawText`（若有） |
| 识别出零条 | 提供「保存为一条记录」，不静默丢弃截图 |
| 截图里的时间已过 | 保留原时间并标黄；用户在预览里改时间后再生成 |
| 重复导入同一张图 | SHA-256 命中复用已有 BLOB，只新增关联，不重复占空间 |
| 图片超过 8MB / 非图片 | 拒绝并提示压缩或截图后重试；后端按魔数校验，不信任扩展名 |
| 多条待办共用一张图 | 只存一份文件 + 多条关联；删除单条只解除关联 |
| 数据库体积增长 | 单图 8MB 上限、内容去重、孤儿清理；备份文件会随图片变大，诊断页显示附件占用 |
| 剪贴板取不到图片 | 提示改用拖拽或「选择文件」，不阻断手动录入 |
| 多实例 | `tauri-plugin-single-instance` 已保证单实例，调度器不会重复运行 |
| 时区 / 夏令时 | 存 UTC 毫秒，按本地日历推进；跨时区查看时按当前本地时间展示 |
| 数据备份 / 恢复 | 整库快照，`notes` 与图片 BLOB 自动包含；恢复后图片仍可预览 |

## 8. 测试计划

### 8.1 Rust 单元与集成测试

在 `src-tauri/src/notes/tests.rs` 中沿用临时数据库模式：

1. 新建 / 更新 / 删除 / 恢复 / 清空已完成，列表与详情字段一致。
2. `today` / `week` / `all` / `done` 筛选边界（本地时区 00:00 与周一 00:00），确认 today / week 包含逾期。
3. 关键字搜索大小写、`%` / `_` 转义、标签精确匹配。
4. 保存时 `remind_at = due_at - lead`；清空截止时联动清空提醒与重复。
5. 一次性提醒：到期被选中一次，第二次不再选中。
6. 稍后提醒：`snooze_until` 不早于 `remind_at`，触发后清空；一次性条目不再提醒。
7. 重复 + 稍后提醒：首次触发后 `remind_at` 已推进，稍后到点不会再推进，下一次不会丢失。
8. 重复提醒：触发后 `remind_at` 前进到未来；应用停机多个周期后一次跳到未来。
9. 重复规则：每天、工作日（周五 → 周一）、每周、每月；1 月 31 日 → 2 月 28/29 日；闰年 2 月 29 日 → 3 月 29 日。
10. 重复条目完成：顺延且保持 `open`，返回 `rolledTo`；提前完成只顺延一次，不跳过下一次；已触发后完成不再推进。
11. 一次性条目完成：进入 `done`，调度不再命中；取消完成恢复。
12. 软删除：列表与计数排除；恢复按 `deleted_from_status` 还原（重复条目完成一次后删除再恢复仍是 `open`）；清空已完成为软删除；30 天清理。
13. 暂停 / 免打扰 / 通知开关关闭：期间不查、不标记；结束后一次补报；免打扰跨零点时段判断正确。
14. 数据库迁移：v4 库补 `done_at` / `deleted_at` / `deleted_from_status` 与 `ai_providers.supports_vision` 后版本为 5；v5 库补 `notes.kind` 并把带截止的旧 `note` 归一化到 `todo` 后版本为 6；重复初始化幂等；索引在补列后才创建。
15. 备份：`VACUUM INTO` 快照中存在 `notes` 表与数据；`note_files` 的 BLOB 可完整读回。
16. 附件入库：同图两次导入只产生一条 `note_files`，关联数正确；删除关联后归零才删文件。
17. 图片校验：PNG / JPEG / WebP 通过，伪造扩展名、超 8MB、单条超 5 张被拒绝。
18. `notes_create_from_image`：N 条待办 + 文件 + 关联在同一事务；中途任务非法时整体回滚，无残留行。
19. 孤儿清理：软删除超期清理后，无关联图片被删除，仍被引用的保留。
20. 识别结果解析：代码围栏、`dueAt` / `due_at`、ISO 与本地时间、越界时间、空数组、非法 JSON。
21. 视觉请求组装：openai / anthropic / gemini 三种 body 的图片字段正确；超时、401、模型不支持图片的错误映射为中文。
22. 提示词与用量：识别调用写入 `ai_usage_logs`，日志与错误信息不含 base64。
23. 未到期条目 snooze：`snooze_until = max(now + minutes, remind_at)`，不会提前消耗一次性提醒；已完成 / 已删除条目被拒绝。
24. 重复条目完成后删除再恢复：按 `deleted_from_status` 还原为 `open`，不因 `done_at` 非空变成 `done`。
25. 清空已完成：全部转为 `deleted` 且 `deleted_from_status = 'done'`，计数排除；30 天物理清理后触发孤儿图片回收。
26. 通知开关关闭期间到期：不写 `notified_at`；重新打开后合并补报一次。
27. `notes_tags` 聚合：中英文逗号、空标签、大小写与重复标签的计数正确。
28. `supports_vision`：默认 0，标记后截图识别才选中；未标记的服务商被显式指定时返回中文错误。
29. 改期重提醒：一次性条目提醒过一次后修改 `due_at` / 提前量 / 重复，`notified_at` 被清空，新时间到达时能再次命中。
30. 逾期条目首次触发前 snooze 后完成重复条目：`remind_at` 仍在过去，完成后必须推进到下一次未来时间，不能保持逾期。
31. 重复条目触发后 snooze：`snooze_until = now + minutes`，到点补提醒且 `remind_at` 不变；一次性条目仍取 `max`。
32. 无截止条目出现在 `today` / `week` 列表，且排序在「无截止」组内符合置顶 → 优先级 → 更新时间。
33. 附件路径导入：`path` 与 `base64` 两种入口得到同一 `sha256` 行；超 8MB、伪造扩展名被拒绝；`notes_attachment_save` 写出的字节与入库一致。
34. 选择文件不经过 base64：IPC 入参只有 `path` 时也能入库并识别（识别仍用压缩副本）。
35. 类型：不传 `kind` 默认 `todo`；创建 `idea` 后按 `kind` 过滤只返回该类型；更新为 `reference` 后筛选同步；未知值回落 `todo`。
36. 稍后到点：`snooze_until` 到点后再次命中并清空；已通知的一次性条目 snooze 后能重新提醒一次。

### 8.2 前端测试

- `notesQuickParse.ts` 表驱动：相对日期、星期、具体日期、时刻、重复、标签、优先级、歧义写法、已过时间。
- `notesGrouping.ts`：逾期 / 今天 / 之后 / 无截止分组，跨月与跨年边界。
- 组件测试（如现有测试基础设施允许）：快速记录回车提交、输入法组合不提交、编辑器防抖保存、完成失败回滚、删除撤销。
- store：`notes:changed` 后计数节流刷新、暂停状态合并。
- `notesImage.ts`：类型校验、长边压缩保持比例、超限拒绝、data URL 转换。
- `notesTags.ts`：同一标签稳定映射同一色调、色调落在 0–7 范围内、不同标签能分散到不同色调。
- `NotesImportDialog`：解析结果全选 / 反选、编辑标题与时间、空结果转单条、生成失败保留内容。
- `NotesImageViewer`：按需加载、切换与关闭释放 URL、多图共享同一附件不重复请求。

### 8.3 手工验收

1. 新建记录，重启应用后仍在。
2. 设置 1 分钟后到期，隐藏窗口到托盘，确认主窗口被自动拉起并弹出应用内提醒卡片，不再出现 PowerShell 系统通知。
3. 在提醒卡片里点「完成」，条目进入已完成；点「稍后 10 分钟」，`snooze_until` 正确。
4. 设置每天 / 工作日 / 每周 / 每月重复，完成后确认顺延正确；本月 31 日每月重复确认下月取最后一天。
5. 全局 `Ctrl+Alt+N` 在浏览器前台唤起迷你窗口，输入「明天10点 交周报 #工作」回车保存，主窗口角标与列表更新。
6. 迷你窗口 `Esc` 后再次唤起，草稿仍在；输入法输入中文时回车不误提交。
7. 暂停提醒 1 小时与暂停今天，期间到期不弹卡片；恢复后只弹一条聚合提醒。
8. 设置免打扰时段覆盖当前时间，确认不弹；结束后补报一次。
9. 删除记录后 6 秒内撤销，记录回到原位置；30 天后确认物理清理。
10. 关闭应用（托盘退出）跨过提醒时间，再启动确认补报一次。
11. 备份 → 恢复 → 记事数据与图片附件完整，预览正常。
12. 点击任意条目从右侧弹出抽屉，宽度约窗口 50%；新建 / 编辑 / 查看详情都在同一抽屉，关闭后回到列表；窄窗口（< 1100px）抽屉收窄且不溢出；深色 / 青色主题下标签胶囊、提醒卡片、角标、逾期颜色可读。
13. 从微信 / 网页截图后 `Ctrl+V` 粘贴，识别出 3–5 条待办，修改其中一条时间后批量生成；每条都能打开原图。
14. 未配置视觉服务商时入口给出引导；配置一个纯文本模型时提示不支持图片，不丢失截图。
15. 识别中断网 / 超时：保留图片与已编辑内容，重试成功后正常生成。
16. 同一张图导入两次，数据库附件体积不翻倍；删除一条共用图片的待办，其他待办仍可看原图。
17. 识别结果为空时「保存为一条记录」，正文保留识别文字。
18. 重复任务完成一次后删除，再恢复，确认仍是未完成并保留下一次提醒。
19. 未到期任务不显示「稍后」按钮；已到期显示，点击后提醒时间不早于原截止。
20. 关闭「到点提醒」开关，跨过到期时间再打开，确认只补一条聚合提醒，且期间没有写已通知标记。
21. 清空已完成后列表与计数为空；30 天后（或临时改短阈值）确认条目与关联图片被清理。
22. 不写日期直接记一条，保存后仍出现在「今天」列表末尾的「无截止」组。
23. 设置 1 分钟后到期并收到提醒，改到 2 分钟后再次收到提醒（验证 `notified_at` 被重置）。
24. 重复任务触发后在应用内提醒卡片里点「稍后 10 分钟」，10 分钟后再次提醒，下个周期不跳。
25. 在迷你窗口按 Ctrl+1 / Ctrl+K，确认迷你窗口不导航、不打开命令面板。
26. 「+ 新建▾」分别选待办 / 笔记 / 灵感 / 资料，创建的条目类型正确、图标与左栏类型筛选一致；在抽屉里切换类型并保存后列表同步。
27. 点「稍后 10 分钟」：列表出现「已稍后 …」、抽屉提示变为「下次提醒 …（已稍后）」；到点后再次弹提醒卡片，稍后标记消失，一次性条目不会重复提醒第二次。

## 9. 实施拆分

| 阶段 | 内容 | 产出 |
|---|---|---|
| P0 | 数据库迁移（`notes` 三列 + `ai_providers.supports_vision`）、`notes` 领域层、附件表 CRUD、IPC 命令、软删除与清理、Rust 测试 | 可用的 IPC 与测试 |
| P1 | 类型 / API / store、记事本页面、快速记录解析、编辑器（含粘贴图片）、键盘操作 | 可日常录入与管理的完整页面 |
| P2 | 调度器、应用内提醒卡片、暂停 / 免打扰、提醒宿主、导航角标、工作台卡片 | 到期提醒闭环 |
| P3 | 视觉调用、提示词、服务商「支持图片识别」勾选、导入弹窗、识别结果编辑、附件条与全屏预览 | 截图转待办闭环 |
| P4 | 迷你窗口、全局快捷键、托盘菜单、`notes:changed` 跨窗口刷新 | 随时快速记录 |
| P5 | 用户文档、skill、README 能力表、航仔工具（可选）、手工验收 | 可交付版本 |

P0 与 P1 可并行推进；P2 依赖 P0 的调度查询与 P1 的 store；P3 依赖 P0 的附件表与 P1 的编辑器；P4 依赖 P0 的创建命令与 P1 的解析器；P5 在功能验收后补齐。P3 可与 P2 并行，二者互不依赖。

航仔工具作为 P5 可选项：`notes_create`、`notes_search`、`notes_toggle`、`notes_snooze`，写操作沿用现有确认卡，不阻塞核心交付。

## 10. 风险与取舍

1. **只保留应用内提醒卡片**：不接系统通知，避免开发模式下 Toast 被 Windows 显示成「Windows PowerShell」这类陌生署名，也省掉授权、失败重试与各平台差异。代价是提醒时必须有一个可见窗口，因此到点前若主窗口藏在托盘或最小化，Rust 会先把它拉起来。
2. **应用完全退出后无法提醒**：不注册系统计划任务，符合「本地、轻量、无后台常驻服务」的定位；文档需明确。
3. **提醒卡片不占用系统通知中心**：关掉卡片就没了，不能事后在通知中心回看；列表、角标、工作台卡片仍是持久入口，卡片内提供「打开」直接跳到条目。
4. **30 秒轮询粒度**：提醒最多晚 30 秒；对记事本场景足够，换来实现简单、无长连接。
5. **类型只是标签，不拆双模式**：待办 / 笔记 / 灵感 / 资料共用一个列表与同一套完成、提醒机制，降低理解成本；代价是纯记录也会出现在未完成列表，靠类型筛选与完成状态管理。
6. **重复任务完成即顺延**：符合待办工具习惯，但「完成」不再意味着彻底结束；需要终止时先关闭重复，UI 需给出明确提示。
7. **新增一个插件**：只保留 global-shortcut（官方插件）用于任意界面唤起速记；若不需要全局快捷键，可关闭开关，但依赖仍会打包。notification 插件已移除，`Cargo.toml`、`lib.rs`、capability 与前端依赖中都不再有它。
8. **自然语言解析可能误判**：采用保守规则 + 保存前预览；无法确定时不解析，剩余文字原样保留。
9. **迷你窗口的中文输入法行为**：需要在 Windows / macOS 实机验证组合输入与焦点切换，纳入手工验收。
10. **软删除无回收站页面**：只在删除后 6 秒内提供撤销，30 天后自动清理；彻底找回依赖备份。
11. **视觉识别依赖外部模型**：效果取决于所选模型，且无法离线使用。v1 不做本地 OCR；识别结果只进预览、确认后才落库，降低误识别成本。
12. **隐私与费用**：截图中可能包含聊天记录、手机号等敏感信息。只有用户点击「开始识别」才发送，弹窗明确显示服务商；日志不记录 base64；按图片调用计费，前端压缩长边降低 token。
13. **服务商协议差异**：OpenAI 兼容网关对 `image_url` 的支持不一致，v1 只保证 openai / anthropic / gemini 三种 kind；Ollama 的 `images` 字段暂不单独适配，失败时提示换模型。
14. **数据库体积与备份耗时**：图片以 BLOB 入库会让库文件和 `VACUUM INTO` 变慢。用 8MB 上限、SHA-256 去重、孤儿清理和单条 5 张限制控制；删除不会自动缩小文件，清理释放超过 32MB 时补一次 `VACUUM`；诊断页展示附件占用，必要时用户可删图或改用独立数据目录。
15. **每月日期钳制会丢锚点**：1 月 31 日 → 2 月 28 日后，后续都按 28 日推进；保留原始日需要额外存 `repeat_day`，v1 不做，文档与界面需说明。
16. **图片走 base64 IPC**：8MB 原图约 11MB JSON，单张预览有解析与内存开销；先用按需加载控制，如实测卡顿再改 `tauri::ipc::Response` 二进制响应。

## 11. 验收标准

- 无截止时间的记录可快速创建、编辑、搜索、置顶、完成、删除、恢复。
- 默认「今天」视图包含逾期与今天到期，打开即可看到当天要做的事。
- 有截止时间的记录在到点后 30 秒内弹出一次应用内提醒卡片；一次性提醒不重复；修改截止时间后按新时间重新提醒；全程不出现系统通知或 PowerShell 弹窗。
- 无截止的记录在「今天 / 本周 / 全部」里都能看到，位于「无截止」分组。
- 重复提醒按本地日历推进，工作日、月末与闰年边界正确。
- 稍后提醒只在已到期（含逾期）或已进入提前量窗口时提供，且不会早于原提醒；列表与抽屉立刻显示新的触发时间，到点再次提醒；触发后一次性条目结束、重复条目继续落到下一次，不跳期。
- 新建时可选择待办 / 笔记 / 灵感 / 资料，左栏可按类型筛选，列表用图标区分；点击条目或新建都在右侧约 50% 宽抽屉中完成查看与编辑。
- 暂停、免打扰与通知开关关闭期间不打扰、不写已通知标记，结束后只补一条聚合提醒。
- 重复条目完成一次后删除再恢复仍为未完成；清空已完成是软删除，30 天后物理清理。
- 全局快捷键可在任意应用前台唤起迷你窗口，回车保存后主窗口角标与列表同步。
- 侧边栏角标与工作台卡片数量与列表一致。
- 隐藏到托盘或最小化后到点，主窗口被拉起并显示提醒卡片；退出应用后下次启动补报一次。
- 一次性完成与删除可撤销；重复完成按规则顺延，不做撤销；软删除 30 天后清理。
- 粘贴或选择截图后能识别出可编辑的待办清单，确认后一次生成多条，原图挂在每条上并可全屏查看。
- 未配置视觉模型、模型不支持图片、超时或断网时都有明确提示，截图与已编辑内容不丢失。
- 图片类型与大小受限，同图重复导入不重复占空间；删除单条待办不影响其他条目的同图预览。
- 数据库从 v4 升级到 v5 无损，重复启动幂等；备份 / 恢复包含记事数据与图片附件。
- `cargo test`、`cargo clippy -D warnings`、`vue-tsc --noEmit`、`vitest run`、`npm run build` 全部通过（clippy 先记录现有基线，本功能不新增告警）。
