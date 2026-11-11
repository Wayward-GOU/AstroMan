//! 核心业务 service 层
//!
//! 技术方案 0.2 / 2.3：新增能力先落纯 Rust 函数，Tauri 命令与 CLI 只做薄适配。
//! 这里按领域拆分，命令层保留原签名转调，便于后续 CLI / E2E 复用。

pub mod ai_policy;
pub mod audit;
pub mod backup_crypto;
pub mod backup_target;
pub mod contract;
pub mod diagnostics;
pub mod docs;
pub mod docs_site;
pub mod faker;
pub mod graphql;
pub mod mock;
pub mod openapi;
pub mod paths;
pub mod policy;
pub mod profile;
pub mod realtime;
pub mod request;
pub mod revision;
pub mod workspace;
