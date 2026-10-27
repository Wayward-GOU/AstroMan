//! 本机网卡与 IP 地址采集
//!
//! - Windows：通过 `GetAdaptersAddresses` 读取每个网卡的 IPv4 / IPv6 / 网关
//! - macOS / Linux：通过 `getifaddrs` 读取地址与 MAC，并从系统路由表补齐默认网关
//! - 其他平台：返回空列表，由前端做降级展示

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
        sort_adapters(&mut list);
        list
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        unix::adapters()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

/// 已连接的网卡优先，其次地址多的优先
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn sort_adapters(list: &mut [AdapterIp]) {
    list.sort_by(|a, b| {
        b.up.cmp(&a.up)
            .then(b.ipv4.len().cmp(&a.ipv4.len()))
            .then(a.name.cmp(&b.name))
    });
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
#[cfg(windows)]
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

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

/// macOS / Linux：`getifaddrs` + 系统路由表
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix {
    use super::{format_mac, sort_adapters, AdapterIp};
    use std::collections::HashMap;
    use std::ffi::CStr;
    use std::net::{Ipv4Addr, Ipv6Addr};

    pub fn adapters() -> Vec<AdapterIp> {
        let mut map: HashMap<String, AdapterIp> = HashMap::new();

        unsafe {
            let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
            if libc::getifaddrs(&mut head) != 0 {
                return Vec::new();
            }

            let mut cursor = head;
            while !cursor.is_null() {
                let entry = &*cursor;
                let name = CStr::from_ptr(entry.ifa_name).to_string_lossy().to_string();
                let up = entry.ifa_flags & libc::IFF_UP as u32 != 0;

                let item = map.entry(name.clone()).or_insert_with(|| AdapterIp {
                    name: name.clone(),
                    mac: String::new(),
                    ipv4: Vec::new(),
                    ipv6: Vec::new(),
                    gateway: Vec::new(),
                    up,
                });
                if up {
                    item.up = true;
                }

                if !entry.ifa_addr.is_null() {
                    let family = (*entry.ifa_addr).sa_family as i32;
                    match family {
                        libc::AF_INET => {
                            if let Some(address) = ipv4(entry.ifa_addr) {
                                if !item.ipv4.contains(&address) {
                                    item.ipv4.push(address);
                                }
                            }
                        }
                        libc::AF_INET6 => {
                            if let Some(address) = ipv6(entry.ifa_addr) {
                                if !item.ipv6.contains(&address) {
                                    item.ipv6.push(address);
                                }
                            }
                        }
                        #[cfg(target_os = "macos")]
                        libc::AF_LINK => {
                            if item.mac.is_empty() {
                                item.mac = link_mac(entry.ifa_addr);
                            }
                        }
                        #[cfg(target_os = "linux")]
                        libc::AF_PACKET => {
                            if item.mac.is_empty() {
                                item.mac = packet_mac(entry.ifa_addr);
                            }
                        }
                        _ => {}
                    }
                }
                cursor = entry.ifa_next;
            }
            libc::freeifaddrs(head);
        }

        let gateways = default_gateways();
        let mut list: Vec<AdapterIp> = map
            .into_values()
            .filter(|item| !item.ipv4.is_empty() || !item.ipv6.is_empty())
            .map(|mut item| {
                if let Some(gateway) = gateways.get(&item.name) {
                    item.gateway.push(gateway.clone());
                }
                item
            })
            .collect();
        sort_adapters(&mut list);
        list
    }

    unsafe fn ipv4(addr: *const libc::sockaddr) -> Option<String> {
        let raw = (*(addr as *const libc::sockaddr_in)).sin_addr.s_addr;
        Some(Ipv4Addr::from(u32::from_be(raw)).to_string())
    }

    unsafe fn ipv6(addr: *const libc::sockaddr) -> Option<String> {
        let raw = (*(addr as *const libc::sockaddr_in6)).sin6_addr.s6_addr;
        Some(Ipv6Addr::from(raw).to_string())
    }

    /// macOS 的 MAC 地址挂在 AF_LINK 的 sockaddr_dl 上：LLADDR(s) = sdl_data + sdl_nlen
    #[cfg(target_os = "macos")]
    unsafe fn link_mac(addr: *const libc::sockaddr) -> String {
        let link = addr as *const libc::sockaddr_dl;
        let name_len = (*link).sdl_nlen as usize;
        let addr_len = (*link).sdl_alen as usize;
        if addr_len != 6 {
            return String::new();
        }
        let base = (*link).sdl_data.as_ptr() as *const u8;
        format_mac(std::slice::from_raw_parts(base.add(name_len), addr_len))
    }

    /// Linux 的 MAC 地址挂在 AF_PACKET 的 sockaddr_ll 上
    #[cfg(target_os = "linux")]
    unsafe fn packet_mac(addr: *const libc::sockaddr) -> String {
        let link = addr as *const libc::sockaddr_ll;
        if (*link).sll_halen as usize != 6 {
            return String::new();
        }
        format_mac(&(*link).sll_addr[..6])
    }

    /// 默认网关变化不频繁，30 秒内复用上一次的系统路由查询结果
    fn default_gateways() -> HashMap<String, String> {
        use std::sync::{Mutex, OnceLock};
        use std::time::{Duration, Instant};

        static CACHE: OnceLock<Mutex<Option<(Instant, HashMap<String, String>)>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(None));
        let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((at, gateways)) = guard.as_ref() {
            if at.elapsed() < Duration::from_secs(30) {
                return gateways.clone();
            }
        }
        let gateways = query_gateways();
        *guard = Some((Instant::now(), gateways.clone()));
        gateways
    }

    #[cfg(target_os = "macos")]
    fn query_gateways() -> HashMap<String, String> {
        let mut map: HashMap<String, String> = HashMap::new();
        for family in ["inet", "inet6"] {
            let output = match std::process::Command::new("netstat")
                .args(["-rn", "-f", family])
                .output()
            {
                Ok(output) => output,
                Err(_) => continue,
            };
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let cols: Vec<&str> = line.split_whitespace().collect();
                // Destination  Gateway  Flags  Netif
                if cols.len() < 4 || cols[0] != "default" {
                    continue;
                }
                let gateway = cols[1].split('%').next().unwrap_or_default();
                if gateway.is_empty() || gateway.starts_with("link#") {
                    continue;
                }
                map.entry(cols[3].to_string())
                    .or_insert_with(|| gateway.to_string());
            }
        }
        map
    }

    #[cfg(target_os = "linux")]
    fn query_gateways() -> HashMap<String, String> {
        let mut map: HashMap<String, String> = HashMap::new();
        if let Ok(output) = std::process::Command::new("ip")
            .args(["route", "show", "default"])
            .output()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                // default via 192.168.1.1 dev eth0 ...
                let tokens: Vec<&str> = line.split_whitespace().collect();
                let gateway = tokens
                    .iter()
                    .position(|token| *token == "via")
                    .and_then(|index| tokens.get(index + 1));
                let iface = tokens
                    .iter()
                    .position(|token| *token == "dev")
                    .and_then(|index| tokens.get(index + 1));
                if let (Some(gateway), Some(iface)) = (gateway, iface) {
                    map.entry(iface.to_string())
                        .or_insert_with(|| gateway.to_string());
                }
            }
        }
        map
    }
}
