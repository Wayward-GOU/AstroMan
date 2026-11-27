//! PAC 自动接管：只把 LLM 接口域名指到本机代理，其余流量直连。
//!
//! 相比「全局 HTTP/HTTPS 代理」，PAC 有两个硬好处：
//! 1. 浏览器、iCloud、系统更新这些流量完全不经过 MITM，不会因为证书固定而坏掉；
//! 2. 脚本末尾带 `; DIRECT` 兜底——代理进程被强杀时客户端会自动回落直连，
//!    不会出现「系统代理指向死端口导致全机断网」。
//!
//! PAC 由我们自己的极简 HTTP 服务提供（`http://127.0.0.1:<port>/proxy.pac`），
//! 不用落盘、不用管理员权限，端口随机分配，进程退出即失效。

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

/// 精确匹配的 LLM 接口域名。桌面客户端基本都直连这些主机。
const EXACT_HOSTS: &[&str] = &[
    "api.anthropic.com",
    "api.openai.com",
    "api.deepseek.com",
    "api.qnaigc.com",
    "openrouter.ai",
    "api.moonshot.cn",
    "api.siliconflow.cn",
    "api.siliconflow.com",
    "open.bigmodel.cn",
    "dashscope.aliyuncs.com",
    "ark.cn-beijing.volces.com",
    "api.minimax.chat",
    "api.minimaxi.com",
    "api.mistral.ai",
    "api.groq.com",
    "api.x.ai",
    "generativelanguage.googleapis.com",
    "api.together.xyz",
    "api.perplexity.ai",
    "api.cohere.ai",
    "api.z.ai",
    "api.baichuan-ai.com",
    "api.lingyiwanwu.com",
    "api.stepfun.com",
    "api.lkeap.cloud.tencent.com",
];

/// 后缀匹配：覆盖同一家的其它子域（如批量接口、区域端点）。
const SUFFIX_HOSTS: &[&str] = &[
    ".anthropic.com",
    ".deepseek.com",
    ".qnaigc.com",
    ".moonshot.cn",
    ".siliconflow.cn",
    ".bigmodel.cn",
    ".openrouter.ai",
    ".minimaxi.com",
    ".z.ai",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacScope {
    /// 只代理上面列出的域名
    Llm,
    /// 全部走代理（等价于原来的全局接管，但保留 DIRECT 兜底）
    All,
}

impl PacScope {
    pub fn parse(value: &str) -> Self {
        match value {
            "all" => PacScope::All,
            _ => PacScope::Llm,
        }
    }
}

/// 生成 PAC 脚本。注意 JS 版本按 PAC 的窄子集写（var + for，不用箭头函数 / const）。
pub fn script(proxy_port: u16, scope: PacScope) -> String {
    let proxy = format!("PROXY 127.0.0.1:{proxy_port}; DIRECT");
    let mut exact = String::new();
    for host in EXACT_HOSTS {
        exact.push_str(&format!("\"{host}\","));
    }
    let mut suffix = String::new();
    for host in SUFFIX_HOSTS {
        suffix.push_str(&format!("\"{host}\","));
    }
    let fallback = match scope {
        PacScope::All => format!("\"{proxy}\""),
        PacScope::Llm => "\"DIRECT\"".to_string(),
    };
    format!(
        "// AstroMan 自动捕获：仅 LLM 接口走本机代理，其余直连\n\
         function FindProxyForURL(url, host) {{\n\
         \x20 var target = \"{proxy}\";\n\
         \x20 var h = (host || \"\").toLowerCase();\n\
         \x20 if (h === \"\" || h === \"localhost\" || h === \"127.0.0.1\" || h === \"::1\") return \"DIRECT\";\n\
         \x20 var exact = [{exact}];\n\
         \x20 for (var i = 0; i < exact.length; i++) {{\n\
         \x20   if (h === exact[i]) return target;\n\
         \x20 }}\n\
         \x20 var suffix = [{suffix}];\n\
         \x20 for (var j = 0; j < suffix.length; j++) {{\n\
         \x20   var s = suffix[j];\n\
         \x20   if (h.length > s.length && h.substring(h.length - s.length) === s) return target;\n\
         \x20 }}\n\
         \x20 return {fallback};\n\
         }}\n"
    )
}

/// PAC 服务句柄：独立线程 + 单线程 runtime，生命周期跟随抓包会话。
pub struct PacServer {
    port: u16,
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl PacServer {
    /// 在随机端口起一个只服务 PAC 脚本的极小 HTTP 服务。
    pub fn spawn(proxy_port: u16, scope: PacScope) -> Result<Self, String> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("E_PAC_BIND: 绑定 PAC 端口失败 {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("E_PAC_BIND: 读取 PAC 端口失败 {e}"))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("E_PAC_BIND: 设置非阻塞失败 {e}"))?;
        let body = Arc::new(script(proxy_port, scope));
        let (tx, rx) = oneshot::channel();
        let join = std::thread::Builder::new()
            .name("capture-pac".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    let Ok(listener) = TcpListener::from_std(listener) else {
                        return;
                    };
                    serve(listener, body, rx).await
                });
            })
            .map_err(|e| format!("E_PAC_BIND: 启动 PAC 线程失败 {e}"))?;
        Ok(Self {
            port,
            shutdown: Some(tx),
            join: Some(join),
        })
    }

    /// 交给系统的 PAC 地址。
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/proxy.pac", self.port)
    }

    #[cfg(test)]
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for PacServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// PAC 服务主循环：拉到 /proxy.pac 就给脚本，其它路径 404。
pub async fn serve(listener: TcpListener, script: Arc<String>, shutdown: oneshot::Receiver<()>) {
    let mut shutdown = shutdown;
    loop {
        tokio::select! {
            _ = &mut shutdown => return,
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let body = script.clone();
                        tokio::spawn(async move { respond(stream, body).await });
                    }
                    Err(_) => return,
                }
            }
        }
    }
}

/// 读请求头（最多 2 KiB），按路径返回。读不到头就断开，避免占用连接。
async fn respond(mut stream: TcpStream, body: Arc<String>) {
    let mut buf = [0u8; 2048];
    let mut filled = 0usize;
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        if filled == buf.len() || buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        tokio::select! {
            _ = &mut deadline => return,
            read = stream.read(&mut buf[filled..]) => {
                match read {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(_) => return,
                }
            }
        }
    }
    let head = String::from_utf8_lossy(&buf[..filled]);
    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let (status, kind, payload) = if path.starts_with("/proxy.pac") {
        ("200 OK", "application/x-ns-proxy-autoconfig", body.as_str())
    } else {
        ("404 Not Found", "text/plain; charset=utf-8", "")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_covers_providers_and_keeps_direct_fallback() {
        let text = script(8888, PacScope::Llm);
        assert!(text.contains("PROXY 127.0.0.1:8888; DIRECT"));
        assert!(text.contains("\"api.deepseek.com\""));
        assert!(text.contains("\".anthropic.com\""));
        // 兜底必须是 DIRECT，否则代理挂了会断网
        assert!(text.trim_end().ends_with("\"DIRECT\";\n}") || text.contains("return \"DIRECT\";"));
    }

    #[test]
    fn all_scope_proxies_everything() {
        let text = script(9000, PacScope::All);
        assert!(text.contains("return \"PROXY 127.0.0.1:9000; DIRECT\";"));
    }

    #[tokio::test]
    async fn serves_pac_and_404s_other_paths() {
        let mut server = PacServer::spawn(8888, PacScope::Llm).expect("start pac");
        let port = server.port();

        let body = http_get(port, "/proxy.pac").await;
        assert!(body.contains("200 OK"), "unexpected: {body}");
        assert!(body.contains("FindProxyForURL"));

        let missing = http_get(port, "/nope").await;
        assert!(missing.contains("404 Not Found"));
        server.stop();
    }

    async fn http_get(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut out = Vec::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            stream.read_to_end(&mut out),
        )
        .await;
        String::from_utf8_lossy(&out).into_owned()
    }
}
