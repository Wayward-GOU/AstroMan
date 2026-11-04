---
name: astroman-notes
description: AstroMan 记事本与待办模块。修改待办 / 笔记 / 灵感 / 参考四种记录、标签与筛选、50% 详情抽屉、到点提醒与重复、稍后提醒、暂停与免打扰、Ctrl+Alt+N 快速记录、截图识别生成待办、图片附件去重，或排查「不提醒 / 提醒时间不对 / 附件丢失」时使用。触发词：待办、记事本、提醒、稍后提醒、重复提醒、标签、快速记录、截图生成待办、notes。
---

# 记事本与待办（v1.0.9 / M10）

## 一句话定位

本地待办 + 随手记：四种记录类型共用一张 `notes` 表，提醒**只在应用内**弹卡片（不调系统通知），图片按内容哈希去重存 SQLite。没有云同步、没有账号。

## 代码地图

| 层 | 位置 | 职责 |
|---|---|---|
| 领域层 | `src-tauri/src/notes/mod.rs` | CRUD、筛选、标签、计数、设置、重复推进、静默判断 |
| 快速记录 | `src-tauri/src/notes/quick.rs` | `quick-capture` 窗口 + `Ctrl+Alt+N` 全局快捷键 |
| 提醒调度 | `src-tauri/src/notes/reminder.rs` | 启动 8 秒后首次 tick，之后每 30 秒 |
| 附件 | `src-tauri/src/notes/attachment.rs` | 校验、sha256 去重、BLOB 读写、保存到磁盘 |
| 截图识别 | `src-tauri/src/notes/vision.rs` + `src-tauri/src/prompts/notes_extract.md` | 三协议视觉模型 → 结构化待办 |
| IPC | `src-tauri/src/commands/notes.rs` | 全部 `notes_*` 命令，薄封装 |
| 建表 / 迁移 | `src-tauri/src/db/schema.sql`、`db/mod.rs` | DB_VERSION 6，新增三张表 |
| 前端 API / 类型 | `src/api/notes.ts`、`src/types/notes.ts` | `invoke` 封装；类型与 Rust `camelCase` 对齐 |
| 状态 | `src/stores/notes.ts` | 列表、筛选、计数、标签、设置缓存 |
| 工具函数 | `src/utils/notesGrouping.ts`、`notesImage.ts`、`notesQuickParse.ts`、`notesTags.ts` | 分组排序 / 压缩哈希 / 自然语言解析 / 标签配色，全部纯函数 |
| 页面 | `src/views/Notes.vue`、`src/views/QuickCapture.vue` | 三栏主页面与迷你窗口 |
| 组件 | `src/components/notes/*`、`NotesReminderHost.vue` | 编辑抽屉、图片查看、截图导入、全局提醒卡片 |

## 数据模型

```text
notes            标题 / 正文 Markdown / kind / status / priority / tags(JSON) /
                 due_at / remind_at / remind_lead_min / remind_repeat /
                 snooze_until / notified_at / pinned / done_at /
                 deleted_at / deleted_from_status
note_files       sha256 唯一 / file_name / mime / size / data(BLOB)
note_file_links  note_id + file_id（多对多）
```

- `kind`：`todo`（有动作与完成态）、`note`（信息记录）、`idea`（随手想法）、`reference`（链接 / 命令 / 片段）。
- `status`：`open` / `done` / `deleted`，删除是软删除，可撤销恢复；`deleted_from_status` 决定恢复后回到哪个状态。
- `repeat`：`none` / `daily` / `weekdays` / `weekly` / `monthly`。
- `priority`：0–3（0 普通，越大越靠前）。
- 附件只对 `todo` 之外也允许（按需），上传后立即去重；同一图片被多条记录引用只存一份。

## 提醒语义（最容易改坏的地方）

1. **先写库、后通知**：`mark_fired` 在事务里推进 `notified_at` / 重复条目的下一次 `remind_at`，成功后才 `emit("notes:reminder")`。反过来会出现「卡片弹了但没标记，30 秒后再弹一次」。
2. **有效提醒时间** = `snooze_until` 非空时取它，否则取 `remind_at`。查询条件见 `due_notes`。
3. **稍后 10 分钟**必须写 `snooze_until = now + 10min` 并清空 `notified_at`；一次性条目取 `max(now+10min, remind_at)`，重复条目直接取 `now+10min`。只改前端展示不算修好，必须验证到点会再次触发。
4. **暂停 / 免打扰**：`pause_until` 或 `is_quiet_now` 为真时，整轮 tick 直接 return，**不写库**；结束后只补发一条聚合提醒，不逐条补。
5. **完成重复条目**：把下一次 `due_at` / `remind_at` 顺延到未来，状态保持 `open`，并清空 `snooze_until` / `notified_at`；前端 `ToggleResult.rolledTo` 不为空时要提示「已顺延到 X」。
6. **修改截止 / 提前量 / 重复**会重算 `remind_at` 并重置 `snooze_until` 与 `notified_at`；只改标题正文不得清掉提醒状态。
7. 应用完全退出后不会提醒；隐藏到托盘仍会提醒（调度在 Rust 侧）。

## 快速记录

- 快捷键固定 `Ctrl + Alt + N`（`Code::KeyN`）。注册失败（被占用）时把设置里的 `globalShortcut` 回退为 `false` 并提示，不要反复重试注册。
- 窗口 label 固定 `quick-capture`，首次创建必须放到独立线程：`WebviewWindowBuilder::build()` 会阻塞主事件循环，主线程直接建窗会表现为白屏 + 所有 invoke 无响应。
- 解析在 `src/utils/notesQuickParse.ts::parseQuickNote()`：识别 `#标签`、`今天 / 明天 / 后天 / 周X / M月D日 / HH:mm`、`每天 / 工作日 / 每周 / 每月` 与 `!` 优先级。新增语法先补单测。
- 迷你窗口提交后发 `notes:changed`，主页面监听刷新，不要靠轮询。

## 截图识别

- 只列出勾选「支持图片识别」（`supportsVision`）的服务商；未配置时给出去「模型服务」的入口，不要静默失败。
- 上传前用 `notesImage.ts` 压缩一份，原图仍作为附件保存；识别失败不能丢原图。
- 提示词在 `prompts/notes_extract.md`，要求模型只返回 JSON；解析失败要保留 `rawText` 供用户重试，不要把异常文本直接灌进标题。

## UI 约定

- 主页面三栏：左侧类型 + 标签筛选，中间卡片流，右侧 **50% 宽度**详情抽屉；新建与编辑都走抽屉，不要退回居中弹窗。
- 标签颜色由标签名稳定派生（同一标签在侧栏、列表、卡片一致）；不要每次渲染随机取色。
- 卡片操作：完成 / 稍后 / 置顶 / 删除 / 撤销；删除与完成用页面内 undo 条，不弹二次确认。
- 列表是前端静态分页 + 搜索，长列表不要每页打后端。
- 工作台「今日待办」卡片只读 `notes_counts`，点击进 `/notes`；不要在工作台再拉全量列表。
- 空状态文案要区分「没有记录」「当前类型没有记录」「当前标签没有记录」。

## 命令与事件

```text
notes_list / notes_get / notes_create / notes_update / notes_toggle / notes_snooze
notes_delete / notes_restore / notes_clear_done / notes_counts / notes_tags
notes_settings_get / notes_settings_set / notes_quick_show
notes_attachment_add / notes_attachment_list / notes_attachment_data /
notes_attachment_save / notes_attachment_remove
notes_extract_image / notes_extract_cancel / notes_create_from_image
```

事件：

- `notes:changed`：`{ reason: create|update|toggle|delete|restore|settings, id? }`
- `notes:reminder`：`{ items: [{ id, title, dueAt, overdue }], firedAt }`
- `notes:open`：托盘 / 提醒卡片请求打开记事本页

## 新增 / 修改检查清单

1. 改表结构：同时改 `schema.sql` 与 `db/mod.rs` 迁移，`DB_VERSION` 递增；老库升级不丢数据。
2. 改提醒：补 `src-tauri/src/notes/tests.rs` 的单测（重复顺延、稍后、暂停、免打扰跨午夜）。
3. 改解析：补 `notesQuickParse.spec.ts`；改分组 / 标签：补 `notesGrouping.spec.ts` / `notesTags.spec.ts`。
4. 改前端：确认抽屉宽度仍为半屏、移动端不溢出；提醒卡片不遮挡主操作。
5. 自测顺序：`npx vitest run` → `cd src-tauri && cargo test --lib` → `npm run build`；涉及窗口 / 快捷键必须桌面端手工验一次。
