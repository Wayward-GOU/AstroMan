---
name: astroman-capture
description: AstroMan 抓包模块的开发与使用约定（HTTP/HTTPS 录制、根证书、系统代理、手机抓包、HAR / ZIP 导出、改参重发）。触发词：抓包、HTTPS 解密、根证书、CA、系统代理、手机代理、微信小程序、HAR、ZIP、压缩包、导出、重发、改包。
---

# 抓包模块（Charles 式流量分析）

抓包是**本机 MITM 代理**：`hudsucker` 在独立线程 + 独立 tokio runtime 上监听端口，解密 HTTPS、录制请求/响应，前端只通过 IPC 读取内存里的流量。数据**不写 SQLite**，重启应用即清空。

页面入口：侧边栏「抓包」（`/capture`，`name: 'capture'`）。

## 1. 文件地图

| 位置 | 职责 |
|---|---|
| `src-tauri/src/capture/mod.rs` | `CaptureManager`、状态机、端口绑定、代理线程生命周期 |
| `src-tauri/src/capture/proxy.rs` | hudsucker 服务装配、CONNECT 隧道、上游代理 |
| `src-tauri/src/capture/handler.rs` | 请求/响应录制管道、body 截断与解码、静态资源过滤 |
| `src-tauri/src/capture/store.rs` | 内存流量仓库（5000 条 / 128 MB body）、分组与查询 |
| `src-tauri/src/capture/ca.rs` | 根证书生成、信任检测、一键安装、导出 |
| `src-tauri/src/capture/sysproxy.rs` | Windows 系统代理接管/还原、崩溃残留清理 |
| `src-tauri/src/capture/har.rs` | HAR 1.2 渲染与导出 |
| `src-tauri/src/capture/archive.rs` | ZIP 导出：index.html、HAR、summary.json、flows 明细 |
| `src-tauri/src/capture/tests.rs` | 代理链路与重发的 Rust 单测 |
| `src-tauri/src/commands/capture.rs` | 全部 IPC 命令，注册在 `lib.rs::generate_handler!` |
| `src/api/capture.ts` | 前端 invoke 封装 |
| `src/types/capture.ts` | 与 Rust camelCase 对齐的类型 |
| `src/views/Capture.vue` | 列表、详情、设置抽屉、分组右键菜单、重发弹窗 |

## 2. 状态机与代理生命周期

```text
idle → starting → running → stopping → idle
                       ↘ error
```

- 启动：`capture_start`。默认端口 **8888**，被占用时向上扫描 **10** 个；端口传 `0` 由系统分配。
- 默认监听 `127.0.0.1`；手机抓包要切 `0.0.0.0`。
- 代理线程名固定 `capture-proxy`，`Runtime` 不能丢在 async 上下文里，也不要挪回 Tauri 主 runtime。
- 启动会清空上一轮流量；状态接口每 1.5 秒轮询一次，前端同时刷新列表。
- 停止：`capture_stop`。先还原系统代理，再发 shutdown 信号并 `join` 线程，最后把端口置 0。
- 代理线程异常退出时会把状态置 `error`，并把原因写进 `lastError`；同时兜底恢复系统代理。

## 3. 录制管道与硬上限

| 常量 | 值 | 行为 |
|---|---|---|
| `MAX_FLOWS` | 5000 | 超出淘汰最旧一条，`dropped` 累加 |
| `MAX_BODY_BYTES` | 128 MB | 所有流量 body 总预算，超出同样淘汰最旧 |
| `MAX_STORED_BODY` | 2 MB | 单条 body 只保留前缀，标记 `truncated` |
| `MAX_COLLECT_BODY` | 8 MB | 超过不缓冲，只转发不录制 body |
| `MAX_IMAGE_BODY` | 1 MB | 图片只对 1 MB 内做 base64 预览 |

- 文本 / 表单 / JSON / XML 直接存 `text`；图片存 `base64`；其它二进制只留 `size` 与 `contentType`。
- `text/event-stream`（SSE）不缓冲 body；压缩响应（gzip / br / deflate）先解码再记录，解不开时按二进制处理。
- `GET` / `HEAD` 不读请求体。
- **静态资源默认也录制**：前端 `captureStart` 固定传 `captureStatic: true`。`captureStatic=false` 时会按扩展名跳过 js/css/图片/字体/音视频。
- `ignorePatterns` 命中 host 或 path 即跳过；`sslHosts` 目前只是契约字段，尚未参与解密决策。前端两项都传空数组，也没有对应设置项——扩展前先补 UI 与 handler。
- `CONNECT` 请求本身不记一条。

## 4. 根证书（HTTPS 解密前提）

证书与私钥落在 `<数据目录>/certs/`：

```text
astroman-ca.crt    PEM 证书
astroman-ca.key    PEM 私钥
astroman-ca.json   创建时间 / 过期时间 / 指纹元数据
```

- `capture_ca_info` 每次都会 `ensure`：不存在则自动生成；同时用系统证书库检测**当前这张证书**是否已信任。
- `capture_ca_generate(force=true)` 重新生成。**旧证书随即失效**，已安装的旧证书必须重新安装。
- `capture_ca_install`：Windows 走 `certutil -user -f -addstore Root`，装进「当前用户 → 受信任的根证书颁发机构」，不需要管理员权限。macOS / Linux 返回「请手动导入」。
- `capture_ca_export(path)`：把 PEM 证书导出到用户选择的位置，供手机或其它设备安装。
- 前端 5 秒轮询一次信任状态，装完证书通常几秒内变「CA 已信任」。
- Firefox 使用独立证书库与代理设置，需要单独导入证书、单独填代理；系统层面信任对它不生效。
- Android 7+ 把用户证书与系统证书分开；App 默认只信系统证书。微信等 App 不认用户证书时，HTTPS 仍会失败。

## 5. 系统代理接管

- 只改 **Windows 当前用户** 的 WinINET：`HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 的 `ProxyEnable` / `ProxyServer`。
- 开启前把原值备份到 `<数据目录>/capture-proxy-backup.json`，关闭时按备份还原；原值为空则删除键值。
- 停止录制、代理线程退出、应用下次启动（`restore_stale`）都会尝试恢复，避免崩溃后系统断网。
- macOS / Linux **不支持一键接管**，命令返回明确错误，需用户手动把系统代理指向 `127.0.0.1:端口`。
- 接管只影响走系统代理的程序；自己指定代理的 App、Firefox、HTTP/3（QUIC）流量不会经过这里。

## 6. 手机抓包

1. 设置抽屉把「监听地址」切到 `0.0.0.0（手机）`，开始录制。
2. 手机与电脑连**同一 Wi-Fi**；手机 Wi-Fi 代理填设置里显示的「手机代理」`局域网IP:端口`（用 `capture_lan_ips` 取带网关的 IPv4，不要填 127.0.0.1）。
3. 手机浏览器打开任意 HTTP 页面确认代理生效，再装根证书：设置抽屉「导出」证书 → 传到手机安装。
4. iOS：安装描述文件后还要到「设置 → 通用 → 关于本机 → 证书信任设置」手动打开完全信任。Android：装到「用户」证书；Android 7+ 的 App 默认不信任用户证书。

**手机像“断网”**：几乎都是根证书没装 / 没信任。MITM 会拒绝所有它无法解密的 HTTPS，表现就是全部请求失败；先装证书，或临时关掉手机代理恢复上网。

**微信小程序边界**：

- 小程序请求走微信自己的网络栈，可能**绕过 Wi-Fi 代理**，也可能**不信任用户证书**；Android 上尤其明显。
- 自己的小程序：用「微信开发者工具 → 设置 → 代理 → 手动设置代理」指向 `127.0.0.1:8888`，并在「详情 → 本地设置」勾选「不校验合法域名、web-view（业务域名）、TLS 版本以及 HTTPS 证书」。开发者工具跑在本机，代理与证书都可用。
- 别人的小程序：优先用 iPhone（系统级信任用户证书后成功率更高）；Android 通常需要 root 把证书装进系统证书目录。
- AstroMan 不做 VPN / TUN 全局劫持，无法强制接管所有 App 流量。

## 7. 列表、详情与导出

- 列表按 **host 分组**，组内按时间倒序；支持关键字过滤（域名 / 路径 / 方法 / 状态）、只看错误、展开 / 折叠、自动滚动。
- 详情页签：概览（方法、URL、状态、耗时、大小、客户端地址）、请求（头 + body）、响应（头 + body）、原始报文。文本 body 可格式化 JSON、复制、另存；图片直接预览；二进制提示大小与类型。
- 导出 HAR：顶部「导出 HAR」= 当前全部流量；域名分组**右键 → 导出该分组 HAR**，只导出该 host 的流量。
- 导出 ZIP：顶部「导出 ZIP」或分组右键「导出该分组 ZIP」，适合把流量直接交给别人查看。解压后结构固定：

  ```text
  astroman-capture-YYYYMMDD-HHmmss/
    index.html          汇总 + 每条流量的请求 / 响应 / 元数据链接
    README.txt          目录说明
    summary.json        条数、域名、请求 / 响应体量汇总
    capture.har         标准 HAR，可导入 DevTools / Charles
    flows/0001-POST-host/
      meta.json         方法、URL、状态、耗时、头、body 元信息
      request.txt       请求行 + 请求头 + 文本 body
      response.txt      状态行 + 响应头 + 文本 body
      response.body.png 图片 / 二进制原始内容（有 base64 时）
  ```

  ZIP 用 `zip` crate 的 deflate 压缩，`archive.rs::export` 写失败会删除半成品。文本 body 只放 `.txt`，不重复写入；图片等 base64 内容额外解码成原始文件。HAR 的渲染统一走 `har::render()`，独立 `.har` 与 ZIP 内 `capture.har` 同源。
- 保存 body：`capture_save_body(id, part, path)`，`part` 只能是 `request` / `response`；文本直接写，base64 先解码。
- 流量只在内存：清空、停止后重新开始、重启应用都会丢失；需要留档及时导出 HAR / ZIP 或保存 body。

## 8. 改参重发

详情页「重发」打开弹窗，可改：**方法、URL、请求头（增删改）、请求体**。

| 场景 | 行为 |
|---|---|
| 录制中 | 请求经本机代理 `127.0.0.1:{port}` 发出，会生成一条**新流量**；成功后自动刷新并选中新记录 |
| 未录制 | 直连目标，响应只显示在「重发结果」弹窗，不写入列表，也不改变录制状态 |

实现要点（`commands/capture.rs::repeat_request`）：

- URL 缺协议时补 `http://`；只接受 `http` / `https`。
- 方法统一转大写并校验。
- 自动剔除 `host`、`content-length`、`connection`、`proxy-connection`、`transfer-encoding`、`upgrade`、`keep-alive`、`content-encoding`，这些由客户端重算。
- 不跟随重定向（3xx 原样展示），超时 120 秒，忽略目标证书错误（抓包场景常见自签名）。
- body 覆盖规则：用户改过才传 `body`；没改则沿用原文本；原 body 是二进制且未改时按 base64 原样发送；二进制且被改动时弹窗禁止编辑并提示。
- 录制中必须把响应体读完，否则新流量的响应可能来不及落库。

## 9. IPC 契约

| 命令 | 入参（前端 camelCase） | 返回 |
|---|---|---|
| `capture_start` | `port?` `bindAddr?` `sslHosts?` `ignorePatterns?` `captureStatic?` `upstreamProxy?` | `CaptureStatus` |
| `capture_stop` | — | `CaptureStatus` |
| `capture_status` | — | `CaptureStatus` |
| `capture_flows` | `limit?` | `CaptureFlow[]` |
| `capture_flow_detail` | `id` | `CaptureFlowDetail` |
| `capture_flow_clear` | — | `CaptureStatus` |
| `capture_lan_ips` | — | `string[]` |
| `capture_ca_info` | — | `CaptureCaInfo` |
| `capture_ca_generate` | `force?` | `CaptureCaInfo` |
| `capture_ca_install` | — | `CaptureCaInfo` |
| `capture_ca_export` | `path` | — |
| `capture_sysproxy_enable` | — | `CaptureStatus` |
| `capture_sysproxy_disable` | — | `CaptureStatus` |
| `capture_repeat` | `id` `method?` `url?` `headers?` `body?` | `RepeatResult` |
| `capture_export_har` | `path` `ids?` | — |
| `capture_export_zip` | `path` `ids?` | — |
| `capture_save_body` | `id` `part` `path` | — |

```ts
const status = await api.captureStart({ port: 8888, bindAddr: '127.0.0.1', captureStatic: true })
const result = await api.captureRepeat({ id: flow.id, method: 'POST', url, headers, body })
```

增删命令时三处一起改：`commands/capture.rs` → `commands/mod.rs`（模块已存在，新增函数不用改）→ `lib.rs::generate_handler!`，再补 `src/api/capture.ts` 与 `src/types/capture.ts`。

## 10. 常见现象对照

| 现象 | 原因 | 处理 |
|---|---|---|
| 列表只有 CONNECT / 没有 HTTPS 内容 | 根证书未信任 | 看顶部「CA 未安装」，一键安装后重启目标 App |
| 装完仍显示未信任 | 重新生成过证书，装的是旧证书 | 再点「重新安装」覆盖 |
| 手机开代理后全部请求失败 | 手机没装 / 没信任 CA | 先装证书；iOS 还要打开「证书信任设置」 |
| 手机填了 `127.0.0.1:8888` 抓不到 | 127.0.0.1 指向手机自己 | 监听切 `0.0.0.0`，代理填电脑局域网 IP |
| 浏览器能抓、命令行走代理抓不到 | 程序没走系统代理 | 显式加 `curl -x http://127.0.0.1:8888`，或手动配置该程序 |
| 某些页面加载不了 | HTTP/3（QUIC）绕过代理，或 App 做了证书固定 | 关闭浏览器 QUIC；pinning 流量无法解密 |
| 微信小程序抓不到 | 微信绕过 Wi-Fi 代理 / 不认用户证书 | 自己的小程序走微信开发者工具；否则用 iPhone，Android 需 root |
| 停止后电脑上不了网 | 系统代理未恢复 | 再点一次「恢复系统代理」，或重启应用触发 `restore_stale` |
| 大响应 / 大文件没有 body | 超过 8 MB 不缓冲，超过 2 MB 只留前缀 | 用「保存 body」前先确认 `truncated` / `binary` |
| 流量突然变少、出现「丢弃」 | 超过 5000 条或 128 MB body | 及时导出 HAR 后清空 |

## 11. 验证

```bash
cd src-tauri && cargo test --lib capture   # 代理链路 + 未录制重发覆盖参数
cd src-tauri && cargo check
npm run build                              # vue-tsc + vite
npm run tauri dev                          # 浏览器模式没有 IPC，必须桌面端验证
```

手工冒烟：开始录制 → 浏览器访问 `http://example.com` 与任意 HTTPS 站点 → 确认两条都进列表且 HTTPS 能看到明文 → 右键分组导出 HAR 与 ZIP → 解压 ZIP 打开 `index.html`，确认 request.txt / response.txt / meta.json 可读、图片能打开 → 在详情里改 URL/body 重发 → 停止录制后再重发一次（应弹「重发结果」且列表不新增）。
