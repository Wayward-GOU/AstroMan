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

---

