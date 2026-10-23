//! 本机网卡与 IP 地址采集（Windows / 其他平台回退）
//!
//! Windows 下通过 `GetAdaptersAddresses` 读取每个网卡的 IPv4 / IPv6 / 网关，
//! 其他平台返回空列表，由前端做降级展示。

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterIp {
    /// 网卡友好名称，例如「以太网」「WLAN」
    pub name: String,
    /// 网卡 MAC 地址，用于与 sysinfo 的接口数据对应
    pub mac: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub gateway: Vec<String>,
    /// 网卡是否处于已连接状态
    pub up: bool,
}

/// 读取本机全部网卡的地址信息
pub fn adapters() -> Vec<AdapterIp> {
    #[cfg(windows)]
    {
        let mut list = unsafe { windows_adapters() };
        list.sort_by(|a, b| {
            b.up.cmp(&a.up)
                .then(b.ipv4.len().cmp(&a.ipv4.len()))
                .then(a.name.cmp(&b.name))
        });
        list
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
unsafe fn windows_adapters() -> Vec<AdapterIp> {
    use std::ptr;
    use windows_sys::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::SOCKADDR;

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64>;

    loop {
        // 用 u64 承载缓冲区，天然满足结构体 8 字节对齐要求
        buffer = vec![0u64; size as usize / 8 + 16];
        let ret = GetAdaptersAddresses(
            0,
            flags,
            ptr::null(),
            buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        );
        if ret == 0 {
            break;
        }
        if ret != ERROR_BUFFER_OVERFLOW {
            return Vec::new();
        }
    }

    let mut list = Vec::new();
    let mut node = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !node.is_null() {
        let adapter = &*node;
        let name = wide_to_string(adapter.FriendlyName);
        let mac_len = (adapter.PhysicalAddressLength as usize).min(8);
        let mac = format_mac(&adapter.PhysicalAddress[..mac_len]);

        let mut ipv4 = Vec::new();
        let mut ipv6 = Vec::new();
        let mut unicast = adapter.FirstUnicastAddress;
        while !unicast.is_null() {
            let entry = &*unicast;
            let sockaddr = entry.Address.lpSockaddr as *const SOCKADDR;
            if let Some((text, is_v4)) =
                sockaddr_to_string(sockaddr, entry.Address.iSockaddrLength)
            {
                let bucket = if is_v4 { &mut ipv4 } else { &mut ipv6 };
                if !bucket.contains(&text) {
                    bucket.push(text);
                }
            }
            unicast = entry.Next;
        }

        let mut gateway = Vec::new();
        let mut gw = adapter.FirstGatewayAddress;
        while !gw.is_null() {
            let entry = &*gw;
            let sockaddr = entry.Address.lpSockaddr as *const SOCKADDR;
            if let Some((text, _)) = sockaddr_to_string(sockaddr, entry.Address.iSockaddrLength) {
                if !gateway.contains(&text) {
                    gateway.push(text);
                }
            }
            gw = entry.Next;
        }

        if !name.is_empty() && (!ipv4.is_empty() || !ipv6.is_empty()) {
            list.push(AdapterIp {
                name,
                mac,
                ipv4,
                ipv6,
                gateway,
                up: adapter.OperStatus == IfOperStatusUp,
            });
        }
        node = adapter.Next;
    }

    list
}

#[cfg(windows)]
unsafe fn sockaddr_to_string(addr: *const windows_sys::Win32::Networking::WinSock::SOCKADDR, len: i32) -> Option<(String, bool)> {
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    if addr.is_null() || len < 8 {
        return None;
    }
    let family = (*addr).sa_family as u16;
    let bytes = std::slice::from_raw_parts(addr as *const u8, len as usize);

    if family == AF_INET && bytes.len() >= 8 {
        return Some((
            format!("{}.{}.{}.{}", bytes[4], bytes[5], bytes[6], bytes[7]),
            true,
        ));
    }
    if family == AF_INET6 && bytes.len() >= 24 {
        let raw = &bytes[8..24];
        let mut segments = [0u16; 8];
        for (index, item) in segments.iter_mut().enumerate() {
            *item = u16::from_be_bytes([raw[index * 2], raw[index * 2 + 1]]);
        }
        return Some((format_ipv6(&segments), false));
    }
    None
}

/// IPv6 文本化：合并最长连续 0 段，兼容 IPv4 映射地址
fn format_ipv6(segments: &[u16; 8]) -> String {
    if segments[..5].iter().all(|v| *v == 0) && segments[5] == 0xffff {
        let b = &segments[6..8];
        return format!(
            "::ffff:{}.{}.{}.{}",
            (b[0] >> 8) as u8,
            (b[0] & 0xff) as u8,
            (b[1] >> 8) as u8,
            (b[1] & 0xff) as u8
        );
    }

    let mut best_start = 0usize;
    let mut best_len = 0usize;
    let mut index = 0usize;
    while index < 8 {
        if segments[index] == 0 {
            let start = index;
            while index < 8 && segments[index] == 0 {
                index += 1;
            }
            let run = index - start;
            if run > best_len {
                best_len = run;
                best_start = start;
            }
        } else {
            index += 1;
        }
    }

    if best_len < 2 {
        return segments
            .iter()
            .map(|v| format!("{v:x}"))
            .collect::<Vec<_>>()
            .join(":");
    }

    let head = segments[..best_start]
        .iter()
        .map(|v| format!("{v:x}"))
        .collect::<Vec<_>>()
        .join(":");
    let tail = segments[best_start + best_len..]
        .iter()
        .map(|v| format!("{v:x}"))
        .collect::<Vec<_>>()
        .join(":");

    if head.is_empty() && tail.is_empty() {
        "::".to_string()
    } else if head.is_empty() {
        format!("::{tail}")
    } else if tail.is_empty() {
        format!("{head}::")
    } else {
        format!("{head}::{tail}")
    }
}

#[cfg(windows)]
fn format_mac(bytes: &[u8]) -> String {
    if bytes.len() != 6 || bytes.iter().all(|b| *b == 0) {
        return String::new();
    }
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(windows)]
fn wide_to_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
            if len > 512 {
                break;
            }
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}
