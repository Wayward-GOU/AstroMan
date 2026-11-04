# AstroMan 开发规范（skill 索引）

本目录每个子目录是一条 skill，入口固定为 `SKILL.md`，frontmatter 的 `name` 必须与目录同名。写代码前按任务挑一条读，比翻全量文档快。

| skill | 什么时候读 | 主要覆盖 |
|---|---|---|
| [`astroman-overview`](astroman-overview/SKILL.md) | 第一次接手项目、找不到文件、确认启动/数据位置 | 目录地图、常用命令、运行时行为、数据位置、必须保留的旧标识 |
| [`astroman-add-feature`](astroman-add-feature/SKILL.md) | 加一个新功能或新接口 | 契约 → 建表/迁移 → command → 注册 → api → 页面 → 验证 |
| [`astroman-backend`](astroman-backend/SKILL.md) | 改 Rust、SQLite、压测引擎、密钥 | 命令规范、迁移铁律、压测要点、主密钥与钥匙串、统计聚合 |
| [`astroman-frontend`](astroman-frontend/SKILL.md) | 改 Vue 页面、样式、脚本引擎、导出 | 页面骨架、设计令牌、CodeArea、AI 抽屉、航仔挂件、常见坑 |
| [`astroman-appearance`](astroman-appearance/SKILL.md) | 改配色、加主题、换航仔形象、调挂件 | 21 套主题与 35 个变量、新增主题流程、对比度指标、贴图与拖拽、线框图标、截图核验 |
| [`astroman-dashboard`](astroman-dashboard/SKILL.md) | 改工作台首页、历史记录页数据大屏、引导与 FAQ 文案 | 首页区块顺序、6 个 KPI、10 步引导 / 18 张技巧卡 / 22 条 FAQ、8 张大屏卡与 SQL 口径 |
| [`astroman-capture`](astroman-capture/SKILL.md) | 改抓包模块，或排查 HTTPS / 手机 / 微信抓不到 | MITM 代理、录制上限、根证书、系统代理、手机抓包、HAR / ZIP 导出、改参重发、断点 / 映射 / 弱网、常见现象对照 |
| [`astroman-notes`](astroman-notes/SKILL.md) | 改记事本 / 待办，或排查不提醒、提醒时间不对、附件丢失 | 四种类型、标签与 50% 抽屉、提醒 / 重复 / 稍后 / 暂停 / 免打扰、快速记录、截图识别、附件去重、三张表 |
| [`astroman-automation`](astroman-automation/SKILL.md) | 改自动化测试子系统的任何一处 | 七张表、执行内核、6 类断言、数据集、Python 钩子、压测门禁、产物与 CLI 退出码 |
| [`astroman-toolbox`](astroman-toolbox/SKILL.md) | 新增 / 修改离线工具 | 92 个工具与 9 分类、`ToolDef` 契约、纯函数 + 组件两种接入、搜索别名、清单同步 |
| [`astroman-ai`](astroman-ai/SKILL.md) | 改航仔（模型接入、工具、提示词、流式） | 双模式、三协议、上下文预算与摘要、92 个工具（含 22 个测试域、7 个抓包域）、确认卡、复盘 |
| [`astroman-testing`](astroman-testing/SKILL.md) | 加测试、改门禁、CI 变红 | vitest、cargo test、tarpaulin 覆盖率、四道门禁、版本号同步、沙箱坑 |
| [`astroman-build-release`](astroman-build-release/SKILL.md) | 打包、换图标、发版 | 构建命令、打包配置、图标体系、发版流程、构建排错 |

维护约定：

- 改代码事实（数量、路径、命令）时同步改对应 skill；**数量以代码为准**，不要照抄旧文档
- 新增一条 skill 时，同时更新本表与 `astroman-overview` 末尾的「配套 skill」表
- 每条 skill 保持「能直接照做」的粒度：命令可复制、参数有值、坑有现象对照
