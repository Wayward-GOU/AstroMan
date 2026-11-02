use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;
use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate,
    RefreshKind, System, UpdateKind,
};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub host_name: String,
    pub uptime_sec: u64,
    pub physical_cores: usize,
    pub logical_cores: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub total: f32,
    pub brand: String,
    pub vendor: String,
    pub frequency_mhz: u64,
    pub cores: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    pub total: u64,
    pub used: u64,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub file_system: String,
    pub total: u64,
    pub available: u64,
    pub percent: f32,
    pub read_per_sec: u64,
    pub write_per_sec: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub name: String,
    pub received_per_sec: u64,
    pub transmitted_per_sec: u64,
    pub total_received: u64,
    pub total_transmitted: u64,
    pub mac: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub gateway: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalIp {
    pub adapter: String,
    pub address: String,
    /// IPv4 / IPv6
    pub family: String,
    pub gateway: String,
    /// 主 IP（用于概览区大字号展示）
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    pub present: bool,
    pub on_ac: bool,
    pub percent: u32,
    pub minutes_left: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    pub name: String,
    pub usage: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub timestamp: i64,
    pub system: SystemInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub swap: Option<SwapInfo>,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInfo>,
    pub ips: Vec<LocalIp>,
    pub battery: Option<BatteryInfo>,
    pub gpus: Vec<GpuDevice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
    /// 占物理内存的百分比（0–100）
    pub memory_percent: f32,
    /// 可执行文件路径，受限权限下可能为空
    pub exe: String,
}

pub struct Monitor {
    sys: System,
    disks: Disks,
    networks: Networks,
    last_tick: Instant,
    prev_net: HashMap<String, (u64, u64)>,
    prev_disk: HashMap<String, (u64, u64)>,
    #[cfg(windows)]
    gpu: Option<gpu::GpuQuery>,
}

pub struct WinStatsState {
    inner: Mutex<Monitor>,
}

// PDH 句柄本身是线程安全的：所有调用都在 Mutex 内串行执行
unsafe impl Send for WinStatsState {}
unsafe impl Sync for WinStatsState {}

impl WinStatsState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Monitor::new()),
        }
    }
}

impl Monitor {
    fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_cpu_usage();

        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last_tick: Instant::now(),
            prev_net: HashMap::new(),
            prev_disk: HashMap::new(),
            #[cfg(windows)]
            gpu: gpu::GpuQuery::new(),
        }
    }

    fn snapshot(&mut self) -> SystemSnapshot {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_tick)
            .as_secs_f64()
            .clamp(0.2, 30.0);
        self.last_tick = now;

        self.sys.refresh_memory();
        self.sys.refresh_cpu_usage();
        self.disks.refresh(true);
        self.networks.refresh(true);

        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let total_swap = self.sys.total_swap();
        let used_swap = self.sys.used_swap();

        let cpus = self.sys.cpus();
        let cores: Vec<f32> = cpus.iter().map(|cpu| cpu.cpu_usage()).collect();
        let brand = cpus
            .first()
            .map(|cpu| cpu.brand().to_string())
            .unwrap_or_default();
        let vendor = cpus
            .first()
            .map(|cpu| cpu.vendor_id().to_string())
            .unwrap_or_default();
        let frequency_mhz = cpus.first().map(|cpu| cpu.frequency()).unwrap_or(0);

        let mut disks = Vec::new();
        for disk in self.disks.list() {
            let usage = disk.usage();
            let key = disk.name().to_string_lossy().to_string();
            let (prev_read, prev_write) = self.prev_disk.get(&key).copied().unwrap_or((0, 0));
            self.prev_disk
                .insert(key, (usage.total_read_bytes, usage.total_written_bytes));
            let total = disk.total_space();
            let available = disk.available_space();
            disks.push(DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount: disk.mount_point().to_string_lossy().to_string(),
                file_system: disk.file_system().to_string_lossy().to_string(),
                total,
                available,
                percent: if total > 0 {
                    (((total - available) as f64 / total as f64) * 100.0) as f32
                } else {
                    0.0
                },
                read_per_sec: ((usage.total_read_bytes.saturating_sub(prev_read)) as f64 / elapsed)
                    as u64,
                write_per_sec: ((usage.total_written_bytes.saturating_sub(prev_write)) as f64
                    / elapsed) as u64,
                removable: disk.is_removable(),
            });
        }
        disks.sort_by_key(|disk| std::cmp::Reverse(disk.total));

        let mut networks = Vec::new();
        let adapters = crate::netinfo::adapters();
        for (name, net) in self.networks.list() {
            let total_rx = net.total_received();
            let total_tx = net.total_transmitted();
            let (prev_rx, prev_tx) = self.prev_net.get(name).copied().unwrap_or((0, 0));
            self.prev_net.insert(name.clone(), (total_rx, total_tx));
            let mac = format_mac(&net.mac_address().0);
            let matched = adapters
                .iter()
                .find(|adapter| !mac.is_empty() && adapter.mac.eq_ignore_ascii_case(&mac))
                .or_else(|| adapters.iter().find(|adapter| adapter.name == *name));
            networks.push(NetworkInfo {
                name: name.clone(),
                received_per_sec: ((total_rx.saturating_sub(prev_rx)) as f64 / elapsed) as u64,
                transmitted_per_sec: ((total_tx.saturating_sub(prev_tx)) as f64 / elapsed) as u64,
                total_received: total_rx,
                total_transmitted: total_tx,
                mac,
                ipv4: matched.map(|a| a.ipv4.clone()).unwrap_or_default(),
                ipv6: matched.map(|a| a.ipv6.clone()).unwrap_or_default(),
                gateway: matched.map(|a| a.gateway.clone()).unwrap_or_default(),
            });
        }
        networks.sort_by(|a, b| {
            (b.received_per_sec + b.transmitted_per_sec)
                .cmp(&(a.received_per_sec + a.transmitted_per_sec))
        });

        let ips = collect_ips(&adapters);

        let swap = if total_swap > 0 {
            Some(SwapInfo {
                total: total_swap,
                used: used_swap,
                percent: percent_of(total_swap, used_swap),
            })
        } else {
            None
        };

        SystemSnapshot {
            timestamp: crate::util::now_ms(),
            system: SystemInfo {
                name: System::name().unwrap_or_default(),
                os_version: System::os_version().unwrap_or_default(),
                kernel_version: System::kernel_version().unwrap_or_default(),
                host_name: System::host_name().unwrap_or_default(),
                uptime_sec: System::uptime(),
                physical_cores: self.sys.physical_core_count().unwrap_or(0),
                logical_cores: cpus.len(),
            },
            cpu: CpuInfo {
                total: self.sys.global_cpu_usage(),
                brand,
                vendor,
                frequency_mhz,
                cores,
            },
            memory: MemoryInfo {
                total: total_mem,
                used: used_mem,
                available: total_mem.saturating_sub(used_mem),
                percent: percent_of(total_mem, used_mem),
            },
            swap,
            disks,
            networks,
            ips,
            battery: battery_info(),
            gpus: self.sample_gpus(),
        }
    }

    fn sample_gpus(&mut self) -> Vec<GpuDevice> {
        #[cfg(windows)]
        {
            match self.gpu.as_ref() {
                Some(query) => query.sample(),
                None => Vec::new(),
            }
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }

    fn top_processes(&mut self, limit: Option<usize>, by_memory: bool) -> Vec<ProcessInfo> {
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet),
        );

        let total_memory = self.sys.total_memory();
        let mut list: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, process)| {
                let memory = process.memory();
                ProcessInfo {
                    pid: pid.as_u32(),
                    name: process.name().to_string_lossy().to_string(),
                    cpu: process.cpu_usage(),
                    memory,
                    memory_percent: if total_memory > 0 {
                        ((memory as f64 / total_memory as f64) * 100.0) as f32
                    } else {
                        0.0
                    },
                    exe: process
                        .exe()
                        .map(|path| path.to_string_lossy().to_string())
                        .unwrap_or_default(),
                }
            })
            .collect();

        if by_memory {
            list.sort_by(|a, b| b.memory.cmp(&a.memory).then(b.cpu.total_cmp(&a.cpu)));
        } else {
            list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
        }
        if let Some(limit) = limit {
            list.truncate(limit.clamp(1, 500));
        }
        list
    }

    fn kill_process(&mut self, pid: u32, expected_name: Option<&str>) -> Result<(), String> {
        if pid == std::process::id() {
            return Err("不能结束 AstroMan 自身进程".to_string());
        }

        let target = Pid::from_u32(pid);
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[target]),
            true,
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        let process = self
            .sys
            .process(target)
            .ok_or_else(|| format!("进程 {pid} 不存在或已退出"))?;
        let name = process.name().to_string_lossy().to_string();

        // 列表到点击之间进程可能退出、PID 可能被复用：名称对不上就拒绝，避免误杀
        if let Some(expected) = expected_name.filter(|value| !value.is_empty()) {
            if !name.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "进程 {pid} 已退出或 PID 已被其他进程占用，请刷新后重试"
                ));
            }
        }

        let display = if name.is_empty() {
            format!("PID {pid}")
        } else {
            format!("{name}（PID {pid}）")
        };

        if is_protected_process(&name, pid) {
            return Err(format!("{display} 是系统关键进程，已阻止结束操作"));
        }

        if process.kill() {
            log::info!("已结束进程 {display}");
            Ok(())
        } else {
            Err(format!(
                "结束 {display} 失败，可能需要管理员权限或进程已受保护"
            ))
        }
    }
}

/// 系统关键进程：结束它们会直接导致蓝屏 / 系统崩溃，直接拒绝
fn is_protected_process(name: &str, pid: u32) -> bool {
    #[cfg(windows)]
    {
        if pid <= 4 {
            return true;
        }
        let lower = name.to_ascii_lowercase();
        const CORE: &[&str] = &[
            "system",
            "registry",
            "smss.exe",
            "csrss.exe",
            "wininit.exe",
            "winlogon.exe",
            "services.exe",
            "lsass.exe",
            "fontdrvhost.exe",
            "memory compression",
            "secure system",
        ];
        if CORE.iter().any(|item| lower == *item) {
            return true;
        }
    }
    #[cfg(target_os = "macos")]
    {
        if pid <= 1 {
            return true;
        }
        if name.eq_ignore_ascii_case("launchd") || name.eq_ignore_ascii_case("kernel_task") {
            return true;
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if pid <= 1 {
            return true;
        }
        if matches!(name, "systemd" | "init" | "kthreadd") {
            return true;
        }
    }
    let _ = (name, pid);
    false
}

fn collect_ips(adapters: &[crate::netinfo::AdapterIp]) -> Vec<LocalIp> {
    let mut list: Vec<LocalIp> = Vec::new();
    for adapter in adapters {
        let gateway = adapter.gateway.first().cloned().unwrap_or_default();
        for address in &adapter.ipv4 {
            list.push(LocalIp {
                adapter: adapter.name.clone(),
                address: address.clone(),
                family: "IPv4".to_string(),
                gateway: gateway.clone(),
                primary: false,
            });
        }
        for address in &adapter.ipv6 {
            list.push(LocalIp {
                adapter: adapter.name.clone(),
                address: address.clone(),
                family: "IPv6".to_string(),
                gateway: gateway.clone(),
                primary: false,
            });
        }
    }

    // 主 IP 优先选择「有网关 + 非回环 + 非链路本地」的 IPv4
    let usable = |item: &LocalIp| {
        item.family == "IPv4"
            && item.address != "127.0.0.1"
            && !item.address.starts_with("169.254.")
    };
    let index = list
        .iter()
        .position(|item| usable(item) && !item.gateway.is_empty())
        .or_else(|| list.iter().position(usable));
    if let Some(index) = index {
        list[index].primary = true;
    }
    list
}

fn percent_of(total: u64, used: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    ((used as f64 / total as f64) * 100.0) as f32
}

fn format_mac(mac: &[u8]) -> String {
    if mac.len() != 6 || mac.iter().all(|b| *b == 0) {
        return String::new();
    }
    mac.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[tauri::command]
pub fn winstats_snapshot(state: State<'_, WinStatsState>) -> SystemSnapshot {
    let mut monitor = state.inner.lock();
    monitor.snapshot()
}

/// 进程列表：`limit` 为 0 或缺省时返回全部进程
#[tauri::command]
pub fn winstats_processes(
    state: State<'_, WinStatsState>,
    limit: Option<usize>,
    by_memory: Option<bool>,
) -> Vec<ProcessInfo> {
    let mut monitor = state.inner.lock();
    let limit = limit.unwrap_or(0);
    monitor.top_processes(
        if limit == 0 { None } else { Some(limit) },
        by_memory.unwrap_or(false),
    )
}

#[tauri::command]
pub fn winstats_process_kill(
    state: State<'_, WinStatsState>,
    pid: u32,
    name: Option<String>,
) -> Result<(), String> {
    let mut monitor = state.inner.lock();
    monitor.kill_process(pid, name.as_deref())
}

fn battery_info() -> Option<BatteryInfo> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

        let mut status = SYSTEM_POWER_STATUS::default();
        let ok = unsafe { GetSystemPowerStatus(&mut status) };
        if ok == 0 {
            return None;
        }
        let present = status.BatteryFlag & 0x80 == 0 && status.BatteryFlag != 255;
        Some(BatteryInfo {
            present,
            on_ac: status.ACLineStatus == 1,
            percent: if status.BatteryLifePercent == 255 {
                0
            } else {
                status.BatteryLifePercent as u32
            },
            minutes_left: if status.BatteryLifeTime == u32::MAX {
                0
            } else {
                status.BatteryLifeTime / 60
            },
        })
    }
    #[cfg(target_os = "macos")]
    {
        macos_battery()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

/// macOS 通过 `pmset -g batt` 读取电池状态
#[cfg(target_os = "macos")]
fn macos_battery() -> Option<BatteryInfo> {
    let output = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| line.contains('%'))?;
    let percent = line
        .split('%')
        .next()?
        .split_whitespace()
        .last()?
        .parse::<u32>()
        .ok()?;
    // 形如「3:21 remaining」「0:00 remaining」「(no estimate)」
    let minutes_left = line
        .split_whitespace()
        .find(|token| token.matches(':').count() == 1)
        .and_then(|token| {
            let (hours, minutes) = token.split_once(':')?;
            Some(hours.parse::<u32>().ok()? * 60 + minutes.parse::<u32>().ok()?)
        })
        .unwrap_or(0);

    Some(BatteryInfo {
        present: true,
        on_ac: text.contains("AC Power"),
        percent: percent.min(100),
        minutes_left,
    })
}

/// 通过 PDH 读取 GPU Engine 利用率，按显卡（LUID）聚合
#[cfg(windows)]
mod gpu {
    use super::GpuDevice;
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
        PDH_MORE_DATA,
    };

    pub struct GpuQuery {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    impl GpuQuery {
        pub fn new() -> Option<Self> {
            let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0"
                .encode_utf16()
                .collect();
            unsafe {
                let mut query: PDH_HQUERY = std::ptr::null_mut();
                if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != ERROR_SUCCESS {
                    return None;
                }
                let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
                if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != ERROR_SUCCESS {
                    PdhCloseQuery(query);
                    return None;
                }
                // 首次采集建立基线
                PdhCollectQueryData(query);
                Some(Self { query, counter })
            }
        }

        pub fn sample(&self) -> Vec<GpuDevice> {
            unsafe {
                if PdhCollectQueryData(self.query) != ERROR_SUCCESS {
                    return Vec::new();
                }

                let mut size: u32 = 0;
                let mut count: u32 = 0;
                let status = PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    std::ptr::null_mut(),
                );
                if status != PDH_MORE_DATA || count == 0 || size == 0 {
                    return Vec::new();
                }

                let capacity = size as usize / size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>() + 1;
                let mut items: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity(capacity);
                let status = PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    items.as_mut_ptr(),
                );
                if status != ERROR_SUCCESS || count == 0 {
                    return Vec::new();
                }
                items.set_len(count as usize);

                // 先按「显卡 + 引擎类型」把各进程的占用累加，再取每张卡上最忙的引擎
                let mut engines: std::collections::HashMap<(String, String), f64> =
                    std::collections::HashMap::new();
                for item in items.iter() {
                    if item.FmtValue.CStatus != 0 {
                        continue;
                    }
                    let value = item.FmtValue.Anonymous.doubleValue;
                    if value.is_nan() {
                        continue;
                    }
                    let name = wide_to_string(item.szName);
                    let (adapter, engine) = split_instance(&name);
                    *engines.entry((adapter, engine)).or_insert(0.0) += value;
                }

                let mut adapters: Vec<(String, f64)> = Vec::new();
                for ((adapter, _), sum) in engines {
                    match adapters.iter_mut().find(|(name, _)| *name == adapter) {
                        Some((_, peak)) => {
                            if sum > *peak {
                                *peak = sum;
                            }
                        }
                        None => adapters.push((adapter, sum)),
                    }
                }

                adapters.sort_by(|a, b| a.0.cmp(&b.0));
                adapters
                    .into_iter()
                    .enumerate()
                    .map(|(index, (_, usage))| GpuDevice {
                        name: format!("GPU {}", index + 1),
                        usage: usage.clamp(0.0, 100.0) as f32,
                    })
                    .collect()
            }
        }
    }

    impl Drop for GpuQuery {
        fn drop(&mut self) {
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    /// 从实例名中拆出显卡标识与引擎类型
    /// 形如 pid_1234_luid_0x00000000_0x0000C4E5_phys_0_eng_0_engtype_3D
    fn split_instance(instance: &str) -> (String, String) {
        let adapter = match instance.find("luid_") {
            Some(start) => match instance[start..].find("_phys") {
                Some(end) => instance[start..start + end].to_string(),
                None => instance[start..].to_string(),
            },
            None => "gpu".to_string(),
        };
        let engine = match instance.find("engtype_") {
            Some(start) => instance[start..].to_string(),
            None => "other".to_string(),
        };
        (adapter, engine)
    }

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
}

#[cfg(test)]
mod process_tests {
    use super::{is_protected_process, Monitor};

    #[test]
    fn protects_system_critical_processes() {
        #[cfg(windows)]
        {
            assert!(is_protected_process("System", 4));
            assert!(is_protected_process("csrss.exe", 500));
            assert!(is_protected_process("LSASS.EXE", 900));
            assert!(!is_protected_process("chrome.exe", 500));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(is_protected_process("systemd", 1));
            assert!(!is_protected_process("chrome", 500));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(is_protected_process("launchd", 1));
            assert!(!is_protected_process("Google Chrome", 500));
        }
    }

    #[test]
    fn process_list_contains_current_process_and_honours_limit() {
        let mut monitor = Monitor::new();
        let all = monitor.top_processes(None, false);
        assert!(all.len() > 1, "应至少枚举到当前进程与系统进程");

        let me = std::process::id();
        let current = all
            .iter()
            .find(|item| item.pid == me)
            .expect("进程列表应包含当前进程");
        assert!(current.memory > 0, "当前进程内存占用应大于 0");
        assert!(
            (0.0..=100.0).contains(&current.memory_percent),
            "内存占比应在 0–100 之间"
        );

        let limited = monitor.top_processes(Some(3), true);
        assert!(limited.len() <= 3);
        assert!(limited
            .windows(2)
            .all(|pair| pair[0].memory >= pair[1].memory));
    }

    #[test]
    fn kills_spawned_child_process() {
        let mut child = {
            #[cfg(windows)]
            {
                std::process::Command::new("ping")
                    .args(["-n", "60", "127.0.0.1"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .expect("spawn ping")
            }
            #[cfg(not(windows))]
            {
                std::process::Command::new("sleep")
                    .arg("60")
                    .spawn()
                    .expect("spawn sleep")
            }
        };
        let pid = child.id();

        let mut monitor = Monitor::new();
        let mut name = None;
        for _ in 0..20 {
            if let Some(item) = monitor
                .top_processes(None, false)
                .into_iter()
                .find(|item| item.pid == pid)
            {
                name = Some(item.name);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let Some(name) = name else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("未能枚举到子进程 {pid}");
        };

        if let Err(error) = monitor.kill_process(pid, Some(&name)) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("结束子进程失败：{error}");
        }
        child.wait().expect("子进程结束");
    }
}
