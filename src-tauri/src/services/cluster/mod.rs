//! 分布式压测（v1.1.1 一期）
//!
//! 主节点负责配对、调度、时钟对齐、采样合并与报告落库，同时作为施压节点参与压测；
//! 从节点反向连接主节点，复用本机单机压测引擎执行分到的并发。
//!
//! 一期边界：手动 IP + 配对码、单场景同步启动、固定/阶梯并发按权重分摊、
//! 每秒采样汇总、合并报告、失联处理。mDNS、断线重连、跨网段、故障后并发重分配不在本期。

use std::net::{IpAddr, UdpSocket};

pub mod client;
pub mod discovery;
pub mod env_bundle;
pub mod history;
pub mod host;
pub mod merge;
pub mod pool_refresh;
pub mod presence;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod acceptance;

/// 本机可用于配对的地址：优先默认出口网卡的私网地址，再补 127.0.0.1
///
/// UDP `connect` 不发送报文，只让系统选出口地址；拿不到时退回回环，
/// 保证同机自测仍能复制到一个可用地址。
pub fn local_addresses() -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    if let Some(ip) = primary_lan_ip() {
        if is_private(ip) {
            items.push(ip.to_string());
        }
    }
    let loopback = "127.0.0.1".to_string();
    if !items.iter().any(|item| item == &loopback) {
        items.push(loopback);
    }
    items
}

fn primary_lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("223.5.5.5", 80)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// 只接受私网 / 回环 / 链路本地地址：集群端口不对公网开放
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// 池绑定会在运行期注入的变量名
///
/// `column == "*"` 展开为该池的全部列名，否则取 `binding.target`。
/// `columns_of` 由调用方提供：主节点查本机库，从节点读下发的池切片列定义。
/// 这些名字由池在运行期填充，不能算作「需要环境提供」的缺失变量——
/// 否则从节点准备阶段会误报 `本机环境缺少变量：keyword、pageSize…`。
pub(crate) fn pool_injected_names(
    bindings: &[crate::models::PoolBinding],
    columns_of: impl Fn(&str) -> Vec<String>,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for binding in bindings {
        let pool = binding.pool.trim();
        if pool.is_empty() {
            continue;
        }
        if binding.column.trim() == "*" {
            for column in columns_of(pool) {
                push_unique_name(&mut names, &column);
            }
        } else {
            push_unique_name(&mut names, &binding.target);
        }
    }
    names
}

fn push_unique_name(names: &mut Vec<String>, name: &str) {
    let name = name.trim();
    if name.is_empty() || names.iter().any(|item| item == name) {
        return;
    }
    names.push(name.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_address_filter_matches_lan_scope() {
        assert!(is_private("192.168.1.10".parse().unwrap()));
        assert!(is_private("10.0.0.8".parse().unwrap()));
        assert!(is_private("172.16.3.4".parse().unwrap()));
        assert!(is_private("127.0.0.1".parse().unwrap()));
        assert!(is_private("169.254.1.2".parse().unwrap()));
        assert!(!is_private("8.8.8.8".parse().unwrap()));
        assert!(!is_private("2001:4860:4860::8888".parse().unwrap()));
    }

    #[test]
    fn local_addresses_always_include_loopback() {
        let items = local_addresses();
        assert!(items.iter().any(|item| item == "127.0.0.1"));
        assert!(!items.is_empty());
    }

    #[test]
    fn pool_injected_names_expand_star_and_targets() {
        use crate::models::PoolBinding;
        let bindings = vec![
            PoolBinding {
                pool: "accounts".to_string(),
                column: "*".to_string(),
                ..Default::default()
            },
            PoolBinding {
                pool: "tokens".to_string(),
                column: "token".to_string(),
                target: "TOKEN".to_string(),
                ..Default::default()
            },
            PoolBinding {
                pool: "tokens".to_string(),
                column: "token".to_string(),
                target: "TOKEN".to_string(),
                ..Default::default()
            },
        ];
        let names = pool_injected_names(&bindings, |name| {
            if name == "accounts" {
                vec!["keyword".to_string(), "pageSize".to_string()]
            } else {
                Vec::new()
            }
        });
        assert_eq!(names, vec!["keyword", "pageSize", "TOKEN"]);
    }

    #[test]
    fn pool_injected_names_skip_unknown_pool_and_empty_target() {
        use crate::models::PoolBinding;
        let bindings = vec![PoolBinding {
            pool: "missing".to_string(),
            column: "*".to_string(),
            ..Default::default()
        }];
        assert!(pool_injected_names(&bindings, |_| Vec::new()).is_empty());
        let bindings = vec![PoolBinding {
            pool: "p".to_string(),
            column: "value".to_string(),
            target: "  ".to_string(),
            ..Default::default()
        }];
        assert!(pool_injected_names(&bindings, |_| vec!["value".to_string()]).is_empty());
    }
}
