---
name: astroman-add-feature
description: 在 AstroMan 中端到端新增一个功能/接口（Rust command → 前端 api → 页面）。需要加新 Tauri 命令、新数据表、新页面或改 IPC 契约时使用。触发词：新增功能、加个接口、新增命令、新页面、接后端、invoke 不生效。
---

# 新增功能的标准路径

AstroMan 是 Tauri 桌面应用，**任何数据操作都必须走后端 command**，前端不直连数据库、不直接发 HTTP（网络请求由 Rust 的 reqwest 发出）。

按顺序做，缺一步就会"前端调不通"。

## 1. 先定契约

写清楚：命令名（snake_case）、入参（Rust 侧 snake_case，前端传 camelCase）、返回结构。
返回结构若前端要用，必须在 `src/types/models.ts` 里补类型，字段与 Rust 的 `#[serde(rename_all = "camelCase")]` 对齐。

## 2. 数据表（仅新增数据时需要）

修改 `src-tauri/src/db/schema.sql`：

- 新表用 `CREATE TABLE IF NOT EXISTS`
- **老库补列不能写在 schema.sql 里**，要写进 `db/mod.rs` 的 `migrate()` 的 `ALTER TABLE ... ADD COLUMN` 列表（重复执行会报错，代码里用 `let _ =` 忽略）
- **任何依赖新列的索引，必须写在 `migrate()` 的补列之后**，不能留在 schema.sql。历史事故：`CREATE INDEX ... ON test_plans(project_id, updated_at DESC)` 留在 schema.sql 里，老库没有 `updated_at`，启动直接 panic

## 3. Rust command

新建或追加到 `src-tauri/src/commands/<域>.rs`：

```rust
use tauri::State;
use crate::state::AppState;
use crate::util::err;

#[tauri::command]
pub fn demo_do(app: State<'_, AppState>, project_id: i64) -> Result<DemoOut, String> {
    let conn = app.pool.get().map_err(err)?;
    // ... 查询/写入
    Ok(DemoOut { /* ... */ })
}
```

要点：

- 参数名即前端传的 key（`projectId` → `project_id`），Tauri 自动转换
- 需要长耗时/流式任务时用 `async fn` + `AppHandle` 发事件（参考 `commands/stress.rs` 的 `app.emit("stress:progress", &progress)`）
- 错误统一 `Result<_, String>`：IO/DB 用 `util::err`，业务校验用 `format!`/字面量，文案中文且能指导用户
- 取连接：`app.pool.get()`；连接池已开启 WAL、外键、5s busy_timeout

## 4. 注册（两处，必须都改）

1. `src-tauri/src/commands/mod.rs`：`pub mod demo;`
2. `src-tauri/src/lib.rs` 的 `tauri::generate_handler![...]`：加 `commands::demo::demo_do,`

漏掉第 2 步的表现是前端报 `command demo_do not found`。

## 5. 权限

新命令默认走 `core:default`，不需要额外配置。**只有引入新插件**（如 fs、shell、http）才需要改 `src-tauri/capabilities/default.json` 并加对应 `plugin` 到 `Cargo.toml` / `lib.rs`。
现有权限：`core:default`、`core:webview:allow-set-webview-zoom`、`dialog:default`。

## 6. 前端 API 封装

`src/api/index.ts` 按域追加，保持一行一个函数的风格：

```ts
export const demoDo = (projectId: number) =>
  invoke<DemoOut>('demo_do', { projectId })
```

域较大时可拆成 `src/api/project.ts` 这样的小文件，再从 `index.ts` re-export。

## 7. 页面与路由

- 新页面放 `src/views/`，用 `PageHeader` 做标题区
- 路由加在 `src/router/index.ts`（hash 路由）
- 侧边栏导航在 `App.vue` 的 `navs` 数组——**注意 `navs` 必须在命令面板的 `watch` 之前声明**，否则触发 TDZ：`Cannot access 'navs' before initialization`，整个页面白屏
- 需要首页额外露出（统计卡片/图表/功能介绍）时，按 `astroman-dashboard` 的区块顺序与统计口径插入，别在大屏里新开数据表

## 8. 验证

```bash
npm run build   # vue-tsc --noEmit + vite build，能抓类型错误
cd src-tauri && cargo check   # 关闭沙箱执行
npm run tauri dev
```

只跑 `npm run dev`（纯浏览器）时 `invoke` 不存在，属于预期，不要据此判断功能坏了。

前端改动只跑 `npm run build` 不会更新已安装的客户端（安装包内嵌 `dist/`），发版前必须重新执行 `npm run build:win` / `build:mac`，详见 `astroman-build-release`。
