# 常见问题

> 安装与系统兼容性见 [compatibility.md](compatibility.md)，开发环境问题见 [development.md](development.md)。

**1. 提示 `cargo 不是内部或外部命令`**

新环境变量尚未生效。关闭当前终端并重启 IDEA，再执行：

```powershell
where.exe cargo
```

应输出 `D:\Software\Rust\cargo\bin\cargo.exe`。

**2. 依赖下载慢或失败**

在 `D:\Software\Rust\cargo\config.toml` 中配置镜像：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
git-fetch-with-cli = true
```

**3. 打包卡在下载 NSIS / WiX**

Tauri 首次打包会下载安装器工具，需要能访问 GitHub。网络受限时建议先只打 NSIS：

```powershell
npm.cmd run tauri build -- --bundles nsis
```

**4. 端口 1420 被占用**

`vite.config.ts` 设置了 `strictPort: true`，端口被占用会直接失败。关闭占用 1420 端口的进程后重试。

**5. 想恢复初始数据**

关闭应用后删除数据库文件（连同 `-wal`、`-shm`），下次启动会重新建库并写入示例数据：

```text
Windows: %APPDATA%\AstroMan\astroman.db
macOS:   ~/Library/Application Support/AstroMan/astroman.db
```

**6. macOS 提示「无法打开，因为无法验证开发者」或「未打开 AstroMan.app」**

安装包未签名、未公证，属于 Gatekeeper 的正常拦截。**不要点「移到废纸篓」**，用以下任一方式放行：

- 使用 Release 附件里的 `install-macos.sh`（自动下载、安装并移除隔离属性）：`bash install-macos.sh`
- 右键点击 `AstroMan.app` → 打开 → 再次点击「打开」
- 打开「系统设置 → 隐私与安全性」，拉到最下方点击「仍要打开」
- 终端执行 `xattr -dr com.apple.quarantine /Applications/AstroMan.app`

**7. macOS 构建报缺少 SDK / linker**

安装 Xcode Command Line Tools 后重试：

```bash
xcode-select --install
```

若已安装仍失败，执行 `sudo xcode-select --reset` 让工具链指向正确的 Xcode 路径。

**8. macOS 上看不到 GPU 占用**

GPU 利用率依赖 Windows 性能计数器（PDH），macOS 下该模块自动隐藏；CPU、内存、磁盘、网络、电池与本机 IP 均正常展示。

**9. 航仔 用哪个模型？API Key 安全吗？**

航仔 支持三种协议：OpenAI 兼容（DeepSeek / Kimi / 智谱 / 通义千问 / 硅基流动 / OpenRouter / Ollama 等）、Anthropic Claude、Google Gemini。在「航仔 → 模型服务」里填 Base URL、模型与 API Key 即可，模型名建议从下拉列表里选，避免手填错。

API Key 使用 AES-256-GCM 加密后存进本机 SQLite，列表只回显掩码，不会上传到任何服务器；代理、证书、请求数据和聊天记录同样只留在本机。费用按厂商公开单价估算，仅供参考。

**10. 请求 Body 里能写注释吗？**

可以。JSON 类型的 Body 支持 `//` 行注释与 `/* */` 块注释，`Ctrl / ⌘ + /` 可切换选中行的注释；真正发送前会自动剥离注释（压测同样处理），因此服务端收到的仍是合法 JSON。

**11. 抓包能看到 HTTPS，但看不到明文？**

先看顶部「CA 未安装 / CA 已信任」。HTTPS 解密要求目标程序信任 AstroMan 根证书：点「一键安装」（Windows 装进当前用户受信任根；macOS 装进登录钥匙串并设为始终信任），装完重启目标程序；如果重新生成过证书，需要再点一次「重新安装」。Firefox 使用独立证书库，要单独导入；部分 App 做了证书固定（pinning），无法解密。

**12. 手机开了代理就连不上网 / 抓不到微信？**

Android 默认不信任用户证书、部分 App 只信系统证书，先确认手机已安装并信任根证书；iOS 还要到「设置 → 通用 → 关于本机 → 证书信任设置」打开完全信任。微信可能绕过 Wi-Fi 代理，自己的小程序用微信开发者工具设置代理并勾选「不校验合法域名…」，别人的小程序优先用 iPhone，Android 通常需要 root。系统代理填入 `127.0.0.1` 是手机自己，要填电脑局域网 IP。

**13. 抓包流量会保存到数据库吗？**

不会。流量只存在内存，上限 5000 条 / 128 MB body，重启应用即清空；需要留档请导出 HAR / ZIP，或把请求 / 响应 Body 另存为文件。单条 body 最多保留 2 MB 前缀，超过 8 MB 不缓冲。

**14. 断点、Map Local / 弱网没有生效？**

这三项都要先「录制」，并按 URL 规则匹配（区分大小写、支持通配）。断点同时挂起上限 32 条、120 秒未处理会自动放行；Map Local 的本地文件不能超过 50 MB；弱网只影响经代理的流量。规则保存在 `<数据目录>/capture/intercept.json`，抓包页「拦截」里可逐条启停。

**15. 待办到点没有提醒？**

先看「记事本」顶部提醒图标是否处于暂停、免打扰时段，或设置里关了「到点提醒」；关闭后只保留角标，不再弹卡片。提醒由应用内每 30 秒轮询，应用完全退出时不会收到。卡片上「稍后 10 分钟」会改写提醒时间并重新到点提醒。

**16. 能把浏览器抓到的请求导进来吗？**

可以。工作台「导入项目」支持 HAR 1.2（浏览器 DevTools / Charles / Fiddler），按 host 分组建集合，查询串进 Params、表单与 multipart 进 Form；Cookie、Authorization 等敏感头会保留但默认禁用，base64 上传体不会导入。

**17. 数据池怎么按自己的列导入 Excel？**

新建池时先定列（如 `phone,password,token`，列名只能是字母 / 数字 / 下划线），建好后在池详情点「下载 Excel 模板」，按首行表头填数据，首列不能为空，再点「导入 Excel」。导入按表头名匹配，列顺序可以不同，只读第一个 Sheet；缺首列会报错，未知表头会列在 `ignoredColumns`，超过 15 位的数字建议把该列设为文本格式。列可以随时重命名 / 增删 / 排序，重命名会迁移数据，删除列会丢弃该列数据。

**18. 分布式压测下发后提示缺变量或读不到数据池？**

先看集群页「变量体检」：普通变量、密钥、系统变量和数据池会分别列出主节点与各从节点的状态。环境变量由主节点加密下发，默认 `master_wins`；如果切到 `local_wins`，从节点本机同名变量会覆盖主节点值。`{{os:NAME}}` 只注入白名单内的系统变量。数据池不需要在从节点单独配置：主节点在 prepare 时按全局 VU 切片下发，从节点只使用下发内容；开跑后再改池不会影响本次运行。计划里用了自定义列绑定时，版本过低、不支持自定义列的从节点会被拒绝并列出节点名，升级后重新加入即可。单节点池切片上限 64 MB，超限会提示最大来源池，可精简条目或增加从节点。

**19. 粘贴的 JSON 是单引号 / 带全角标点，点「格式化 JSON」提示「已按宽松模式格式化」怎么办？**

说明正文不是标准 JSON，但能被宽松规则读懂，已经帮你改好了。宽松解析覆盖：单引号字符串、尾逗号、无引号属性名、`//` 与 `/* */` 注释、全角标点（`｛｝［］：，＆`、全角空格、`“”‘’`）、BOM 头、尾随分号。提示里会列出修正项，因为规范化会改变实际发出的字节，需要你知道。

如果服务端就是要求接收这种类 JSON 文本（少见），不要点格式化，直接发送——发送前若正文不合规范会弹确认框，可选「仍然发送」。另外，裸写的超长整数（如 19 位 ID）超出 JSON 安全整数范围，会丢精度并单独提示，需要原样传输请存成字符串（加引号）。

**20. Raw 类型的 Content-Type 是怎么定的？**

按内容自动推断：严格 JSON（可含 `{{变量}}`）→ `application/json`，XML → `application/xml; charset=utf-8`，其余与空正文 → `text/plain; charset=utf-8`；`json` / `graphql` 类型固定 `application/json`，`form` 固定 `application/x-www-form-urlencoded`，`binary` 固定 `application/octet-stream`。只要在 Headers 里手写过 Content-Type，就一律不覆盖。规则前后端一致，「实际请求」页签展示的头就是实际发出去的头。

---

