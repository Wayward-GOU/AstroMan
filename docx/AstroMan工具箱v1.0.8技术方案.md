# AstroMan 工具箱 v1.0.8 技术方案

| 项目 | 内容 |
| --- | --- |
| 文档版本 | v1.0.0 |
| 编写日期 | 2026-10-30 |
| 适用版本 | AstroMan v1.0.8（工具箱增强） |
| 状态 | **已实现**（代码、单测、构建均通过） |
| 关联文档 | [AstroMan的本地能力补强技术方案.md](./AstroMan的本地能力补强技术方案.md)（4.13 工具箱 2.0）、[../docs/toolbox.md](../docs/toolbox.md) |

---

## 1. 背景与目标

### 1.1 背景

v1.0.7 的工具箱为 **9 分类 75 个工具**，覆盖 JSON 互转、编解码、文本处理、计算换算、开发辅助、网络与查询手册，形态分 `simple`（声明式输入输出）与 `component`（自定义交互）两种，全部本地计算。

但与本机常备的几款工具对比，仍有两类明显缺口：

1. **通用高频工具缺失**：IT-Tools / DevToys 里几乎人手一个的 TOTP、JWT 验签、JSONPath、JSON 结构化对比、Base58/Base85、Quoted-Printable、文件哈希、图片 Base64、Markdown 预览、ULID，我们一个都没有；
2. **"工具箱"作为产品的能力缺失**：没有收藏、没有最近使用、搜索只有关键词包含匹配，常用工具每次都要翻分类。

而《本地能力补强技术方案》4.13 把 25 个工具整体推到 v1.1.0 / v1.2.0，其中约一半是**纯前端就能做完**的（Base58、Base85、QP、ULID、JSONPath、JSON Schema、JSON diff、造数），没有必要陪着国密、证书、DNS 一起等 Rust 依赖。

### 1.2 目标

| 编号 | 目标 | 衡量标准 |
| --- | --- | --- |
| G-1 | 补齐市场高频缺口 | 新增 17 个工具，全部为市场主流工具箱（IT-Tools / DevToys / CyberChef）的标配项，或国内场景刚需 |
| G-2 | 零新增依赖、零网络请求 | `package.json` 依赖不变；新增代码不发起任何 HTTP 请求，不上传任何输入 |
| G-3 | 复用既有骨架 | 14 个走 `simple` 形态，仅 3 个需要自定义组件；不新增工具框架代码 |
| G-4 | 可测试 | 新增纯函数全部有单测，`vitest` 全量通过 |
| G-5 | 工具箱具备"常用"能力 | 收藏、最近使用、别名搜索三项落地，常用工具 1 次点击可达 |

### 1.3 非目标

- 不做需要 Rust 侧能力的工具（国密 SM2/SM3/SM4、RSA 密钥对、X.509、DNS、TLS 证书链、文件编码探测）——这些留给 v1.1.0，见第 8 节；
- 不改动既有 75 个工具的行为与文案；
- 不引入新的 UI 组件库或图标库；
- 不做 AI 工具化对接（工具箱 → 航仔工具，属另一条线）。

---

## 2. 现状盘点

| 分类 | 数量 | 代表工具 | 明显缺口 |
| --- | --- | --- | --- |
| JSON | 18 | 格式化、互转、实体类生成、JSONL | 无 JSONPath、无结构化 diff、无 Schema |
| 格式化转换 | 1 | 9 种语言美化 | 无 Markdown 预览 |
| 加密编码 | 15 | 哈希/HMAC、AES、JWT 解码签发、Base32/64/Hex | 无 TOTP、无验签、无 Base58/85/QP、无文件哈希 |
| 文本处理 | 13 | 命名、统计、对比、提取、一键处理 | 无脱敏 |
| 计算换算 | 10 | 进制、单位、颜色、时间戳、日期 | 够用 |
| 开发辅助 | 4 | MyBatis、正则、Cron、雪花 | 无造数、无证件校验、无 ULID/NanoID |
| 网络工具 | 5 | IP、子网、URL、端口 | 无 UA 解析、无原始报文解析 |
| 二维码 | 2 | 生成、识别 | 够用 |
| 查询手册 | 7 | ASCII/HTTP/MIME/Header/Git/Docker/正则 | 够用 |

---

## 3. 市场对标

### 3.1 对标对象

| 产品 | 形态 | 特点 | 对我们的意义 |
| --- | --- | --- | --- |
| IT-Tools | Web / 自托管 | 80+ 工具，Vue 3 实现，分类清晰 | 工具全集参照，覆盖面最广 |
| DevToys | 桌面（Win/Mac/Linux） | 30+ 内置工具 + 扩展，Smart Detection | 桌面形态最接近，功能取舍可参照 |
| CyberChef | Web | "网络瑞士军刀"，编码链式编排 | 编码类深度参照 |
| Apifox / Apipost | API 工具内置 | 与接口调试场景强绑定 | 说明"工具箱要贴着调试场景长" |

### 3.2 差距矩阵（只列我们缺的）

| 能力 | IT-Tools | DevToys | 我们 v1.0.7 | 本次处理 |
| --- | --- | --- | --- | --- |
| TOTP / OTP | ✅ | ✅ | ❌ | **新增** |
| JWT 验签（HS/RS/ES） | 仅解析 | 编解码 | 仅解码 | **新增** |
| JSONPath | ✅ | ✅ | ❌ | **新增** |
| JSON 结构化 diff | ✅ | 第三方 | 仅文本 diff | **新增** |
| JSON Schema | 第三方 | ✅ | ❌ | **新增** |
| Base58 / Base85 | ⚠️ 部分 | ✅ | ❌ | **新增** |
| Quoted-Printable | ⚠️ | ✅ | ❌ | **新增** |
| 文件哈希 | ✅ | ✅ | ❌ | **新增** |
| 图片 Base64 | ✅ | ✅ | ❌ | **新增** |
| Markdown 预览 | ✅ | ✅ | ❌ | **新增** |
| ULID / NanoID | ✅ | ✅ | ❌ | **新增** |
| 测试造数 | ✅ | ✅ | ❌ | **新增（含国内证件）** |
| 身份证 / 统一社会信用代码校验 | ❌ | ❌ | ❌ | **新增（差异化）** |
| 文本脱敏 | ❌ | ❌ | ❌ | **新增（差异化）** |
| UA 解析 | ✅ | ✅ | ❌ | **新增** |
| 原始 HTTP 报文解析 | ❌ | ❌ | ❌ | **新增（差异化）** |
| 收藏 / 最近使用 | ⚠️ | ✅ | ❌ | **新增** |
| 国密 SM2/SM3/SM4 | ❌ | ❌ | ❌ | 留 v1.1.0（需 Rust） |
| X.509 / CSR / RSA 密钥对 | ⚠️ | ✅ | ❌ | 留 v1.1.0（需 Rust） |
| DNS / TLS 证书链 | ❌ | ✅ | ❌ | 留 v1.1.0（需 Rust） |

> 结论：**能纯前端做完的高频项，v1.0.8 一次补齐**；需要密码学库与网络探测的，与 v1.1.0 的协议补全一起做，避免半成品。

---

## 4. 选型原则

1. **贴着调试场景**：优先服务"接口调试 → 造数 → 排障 → 贴工单"这条链路（造数、验签、报文解析、脱敏、JSONPath）。
2. **纯前端优先**：能用 WebCrypto / 纯 TS 完成的，不引入 Rust 命令，避免拖慢发版与跨平台验证。
3. **单文件 ≤ 300 行**：每个工具一个纯函数模块，UI 复用 `simple` 骨架，降低维护成本。
4. **可验证**：算法类工具（校验位、TOTP、编码）必须有对照测试向量或往返测试。
5. **离线可信**：不联网、不落盘、不读用户目录，输入只在内存中处理。

---

## 5. 技术方案

### 5.1 架构

```
src/tools/
  jsonpath.ts    JSONPath 解析与求值
  jsondiff.ts    结构化差异
  schema.ts      样例 → JSON Schema
  otp.ts         TOTP / HOTP（WebCrypto HMAC）
  jwtverify.ts   JWT 验签（HS/RS/PS/ES + PEM/JWK）
  encoding.ts    Base58 / Ascii85 / Quoted-Printable
  mask.ts        文本脱敏
  datagen.ts     造数 + 证件校验
  idgen.ts       ULID / NanoID
  netparse.ts    UA 解析 + HTTP 报文解析
src/components/tools/
  FileHashTool.vue     文件哈希（流式 MD5 + SubtleCrypto）
  ImageBase64Tool.vue  图片 ↔ Data URL
  MarkdownTool.vue     Markdown 预览 / HTML 源码
src/stores/
  toolPrefs.ts   收藏与最近使用（localStorage）
```

- **14 个工具走 `simple`**：只写 `run(input, options)`，UI 由 `ToolWorkbench.vue` 统一渲染，零新增界面代码；
- **3 个工具走 `component`**：需要文件读取或富渲染（哈希、图片、Markdown），各自独立组件，懒加载；
- **注册表不变**：继续用 `TOOLS` 数组 + `TOOL_MAP`，只新增 `aliases` 字段（搜索别名）。

### 5.2 新增工具清单（17 个）

| # | 工具 | 分类 | 形态 | 实现要点 |
| --- | --- | --- | --- | --- |
| 1 | JSONPath 查询 | json | simple | 手写词法分析：`.key` / `['key']` / `[0]` / `[*]` / `..key` / `$..*`；输出命中路径 + 合并数组 |
| 2 | JSON 结构化对比 | json | simple | 两段 JSON 用 `---` 分隔；对象按 key 并集、数组按下标逐项比对；输出 `+ / - / ~` 与路径 |
| 3 | JSON 生成 Schema | json | simple | 递归推断 type/properties/required；数组元素合并（字段并集 + 必填交集）；识别 date-time / email / uuid / uri |
| 4 | TOTP / HOTP | crypto | simple | Base32 解码 → HMAC(SHA-1/256/512) → 动态截断；输出当前码、上下周期、剩余秒数、otpauth 链接 |
| 5 | JWT 验签 | crypto | simple | HS 用 HMAC、RS/PS 用 RSA（SPKI PEM 或 JWK）、ES 用 ECDSA（P-256/384/521）；同时校验 exp / nbf / iat |
| 6 | Base58 编解码 | crypto | simple | BigInt 进制转换，保留前导 `1` 零字节；拒绝 0 O I l |
| 7 | Ascii85 编解码 | crypto | simple | 4 字节 → 5 字符，`z` 缩写、`<~ ~>` 定界符、末组补位 |
| 8 | Quoted-Printable | crypto | simple | 编码按 76 列软换行、`=XX` 转义；解码处理软换行与行尾空白 |
| 9 | 文件哈希校验 | crypto | component | 文件按 4MB 分块流式喂 MD5（crypto-js），SHA-1/256/512 走 `crypto.subtle.digest`，CRC32 自实现 |
| 10 | 图片 Base64 | crypto | component | FileReader → Data URL；双向模式，棋盘格底预览，分别复制 Data URL / 纯 Base64 |
| 11 | 文本脱敏 | text | simple | 手机号、身份证、邮箱、银行卡、IP 正则打码；姓名只处理"姓名：张三"这类带标签写法，避免误伤 |
| 12 | Markdown 预览 | format | component | 复用 `renderMarkdown`（html:false + 链接 rel），支持预览 / HTML 源码切换与复制 |
| 13 | 测试数据生成 | dev | simple | 姓名、手机号、身份证（GB 11643 校验位）、银行卡（Luhn）、统一社会信用代码（GB 32100 校验位）、地址、公司、IP；输出文本 / CSV / JSON / SQL |
| 14 | 证件 / 卡号校验 | dev | simple | 自动识别 18 位身份证（校验位 + 出生日期 + 性别 + 行政区划）、USCC、银行卡 Luhn、手机号格式 |
| 15 | ULID / NanoID | dev | simple | Crockford Base32；同批次按毫秒递增，保证字典序与生成顺序一致 |
| 16 | User-Agent 解析 | network | simple | 浏览器（含微信/企微/钉钉/支付宝/curl/Postman）、系统、设备、引擎、架构 |
| 17 | HTTP 原始报文解析 | network | simple | 起始行判定请求/响应，拆头、拆查询参数、Body 预览，并检查缺 Host / Content-Length 等问题 |

### 5.3 平台能力（3 项）

| 能力 | 实现 | 交互 |
| --- | --- | --- |
| 收藏 | `useToolPrefsStore`，`localStorage` key `astroman.tool.favorites`，上限 20 | 工具卡片右上角星标（悬停显示）；详情页标题旁"收藏 / 已收藏" |
| 最近使用 | 同上，key `astroman.tool.recent`，上限 10，打开即记录 | 列表页顶部"最近使用"胶囊，支持一键清空 |
| 别名搜索 | `ToolDef.aliases` 并入 `searchTools` 匹配串 | 搜 `jp` / `打码` / `ua解析` / `b85` 都能命中 |

> 深链已支持：`#/tools?tool=<id>`（`Tools.vue` 读取 query），本次未改动。

### 5.4 安全与边界

| 项 | 处理 |
| --- | --- |
| 数据外发 | 无任何 `fetch` / `invoke` 网络调用，全部本地计算 |
| 大文件 | 哈希按 4MB 分块；图片限制 20MB；生成数量上限 500 条 / ID 100 个 |
| Markdown XSS | 复用既有 `markdown-it({ html: false })`，链接强制 `rel="noopener noreferrer"` |
| 正则性能 | 脱敏正则均为线性结构，无嵌套量词；JSONPath 递归按节点遍历，无回溯风险 |
| 敏感数据落盘 | 收藏/最近只存工具 id，不存输入内容 |

---

## 6. 代码变更清单

### 6.1 新增文件（15）

| 文件 | 行数级别 | 说明 |
| --- | --- | --- |
| `src/tools/jsonpath.ts` | ~200 | JSONPath 解析 + 求值 + 格式化输出 |
| `src/tools/jsondiff.ts` | ~130 | 结构化对比 |
| `src/tools/schema.ts` | ~140 | Schema 推断与合并 |
| `src/tools/otp.ts` | ~120 | TOTP / HOTP |
| `src/tools/jwtverify.ts` | ~190 | JWT 验签 |
| `src/tools/encoding.ts` | ~150 | Base58 / Ascii85 / QP |
| `src/tools/mask.ts` | ~110 | 文本脱敏 |
| `src/tools/datagen.ts` | ~280 | 造数 + 校验 |
| `src/tools/idgen.ts` | ~80 | ULID / NanoID |
| `src/tools/netparse.ts` | ~180 | UA / HTTP 报文 |
| `src/components/tools/FileHashTool.vue` | ~230 | 文件哈希组件 |
| `src/components/tools/ImageBase64Tool.vue` | ~300 | 图片 Base64 组件 |
| `src/components/tools/MarkdownTool.vue` | ~250 | Markdown 预览组件 |
| `src/stores/toolPrefs.ts` | ~70 | 收藏 / 最近使用 |
| `src/tools/__tests__/toolbox.spec.ts` | ~230 | 34 条单测 |

### 6.2 修改文件（3）

| 文件 | 变更 |
| --- | --- |
| `src/tools/types.ts` | `ToolDef` 新增 `aliases?: string[]` |
| `src/tools/registry.ts` | 新增 17 个工具定义；`searchTools` 纳入 `aliases` |
| `src/views/Tools.vue` | 收藏 / 最近使用两组快捷区；卡片星标；详情页收藏按钮；打开即记最近 |

### 6.3 工具数量变化

| 分类 | v1.0.7 | v1.0.8 | 增量 |
| --- | --- | --- | --- |
| JSON | 18 | 21 | +3 |
| 格式化转换 | 1 | 2 | +1 |
| 加密编码 | 15 | 22 | +7 |
| 文本处理 | 13 | 14 | +1 |
| 计算换算 | 10 | 10 | — |
| 开发辅助 | 4 | 7 | +3 |
| 网络工具 | 5 | 7 | +2 |
| 二维码 | 2 | 2 | — |
| 查询手册 | 7 | 7 | — |
| **合计** | **75** | **92** | **+17** |

---

## 7. 验证

### 7.1 自动化

```bash
npx vitest run          # 80 passed（新增 34 条）
npx vue-tsc --noEmit    # 0 error
npx vite build          # 构建通过
```

新增单测覆盖点：

| 模块 | 关键用例 |
| --- | --- |
| JSONPath | 路径取值、通配符、递归下降、无命中提示 |
| JSON diff | 修改/新增/删除识别、缺分隔符报错、完全一致 |
| Schema | 类型与必填、格式识别、数组元素合并 |
| 编码 | Base58 往返与非法字符、Ascii85 往返（含定界符）、QP 往返 |
| 脱敏 | 手机/身份证/邮箱自动打码、单规则隔离、无命中 |
| 造数 | 生成的身份证 / 银行卡 / USCC / 手机号反向校验全部通过；错误校验位被拒 |
| ID | ULID 长度与时间还原、NanoID 长度与字符集 |
| 网络 | Edge/Windows、微信/Android、请求行与状态行解析 |
| TOTP | RFC 6238 SHA-1 官方测试向量（T=59 → 94287082） |
| JWT | HS256 验签通过、篡改后不通过、过期状态 |

### 7.2 手工验收

1. 收藏 3 个工具 → 刷新应用 → 收藏区仍在；取消收藏即时消失；
2. 打开 5 个工具 → "最近使用"按倒序展示，最近 10 个封顶，可清空；
3. 搜索 `jp`、`打码`、`b85`、`ua解析` 均能命中对应工具；
4. 拖入 50MB 文件计算哈希，内存无暴涨、结果与 `certutil -hashfile` 一致；
5. 粘一段真实 JWT + 公钥，验签结论与服务端一致；
6. 造 100 条身份证，抽样在权威校验工具中复核通过。

---

## 8. 后续规划

| 版本 | 内容 | 原因 |
| --- | --- | --- |
| v1.0.9 | 工具箱 → 航仔工具化（让 AI 可调用工具箱） | 与 AI 工具生态线合并 |
| v1.1.0 | SM2/SM3/SM4、RSA 密钥对与加解密、X.509 解析、CSR 自签、JWT RS/ES 签名、DNS 查询、TLS 证书链 | 需要 Rust crate，与"协议补全"同批 |
| v1.1.0 | TOML ↔ JSON/YAML、Protobuf/CBOR 解码、IPv6 子网、chmod 计算、Docker run → Compose | 中等复杂度，优先级次之 |
| v1.2.0 | 文件编码探测（GBK↔UTF-8）、繁简转换、图片压缩、PDF 工具 | 体积或映射表成本较高 |
| Backlog | 数据链路编排（类 CyberChef 的"发送到"流水线）、输入快照、结果导出 | 属工具箱平台化，单独评估 |

---

## 附录 A：本次未做但市场常见的工具（有意取舍）

| 工具 | 不做原因 |
| --- | --- |
| bcrypt / BIP39 / PGP | 需引入较重依赖，桌面端价值低于接口调试场景 |
| MAC 厂商 OUI 查询 | 需内置 3.9 万条 OUI 库（数 MB），收益低于体积成本 |
| Roman numeral / Lorem ipsum / Emoji picker | 与调试场景无关 |
| 图片压缩 / SVG 占位图 | 与"接口工具"定位偏离，v1.2 再议 |
| Docker run → Compose | 文本转换规则多、维护成本高，且非高频 |
| 端口扫描 / 连通性探测 | 涉及网络行为与安全边界，需单独评审 |

## 附录 B：新增工具 ID 速查

`jsonpath` · `json-diff` · `json-schema` · `totp` · `jwt-verify` · `base58` · `ascii85` · `quoted-printable` · `file-hash` · `image-base64` · `mask` · `markdown-preview` · `datagen` · `doc-check` · `ulid` · `ua-parse` · `http-parse`
