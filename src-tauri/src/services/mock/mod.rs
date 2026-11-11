//! G-04 本地 Mock Server
//!
//! 分三层：
//! - `store`：`mock_routes` 表的 CRUD、按请求自动生成路由、响应骨架推导；
//! - `router`：请求匹配与响应构造（手动路由优先，其次真实请求回放，最后实体骨架）；
//! - `server`：进程内 HTTP 服务生命周期、局域网令牌、请求日志环形缓冲。
//!
//! 实现取舍：**不引入 axum / tower-http**，复用文档站（4.4）已经验证过的标准库
//! HTTP 线程（`std::net::TcpListener` + 每连接一线程），发布门禁要求 NSIS 安装包
//! ≤ 25MB，新增大依赖会突破该预算。

pub mod router;
pub mod server;
pub mod store;
