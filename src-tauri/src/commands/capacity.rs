//! 本机施压能力检测（1.7）。
//!
//! 在进程内起一个回环 HTTP 服务（固定 2 字节响应 + keep-alive），按
//! 16/32/…/1024 并发逐档压 3 秒，排除首秒预热后统计 QPS / 延迟 / 错误率，
//! 同时采样 AstroMan 进程的 CPU 与内存占用。结果写 `machine_capacity`，
//! 保留最近 10 次。

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params, OptionalExtension};
use serde_json::json;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, State};

use crate::models::{CapacityResult, CapacitySample};
use crate::state::{AppState, DbPool};
use crate::util::{err, now_ms};

const STAGES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];
const STAGE_SEC: u64 = 3;
const WARMUP_SEC: u64 = 1;
/// 保护上限：正常 21 秒结束，异常时最多 60 秒
const MAX_PROBE_SEC: u64 = 60;
/// CPU 判定口径：进程占用核数 / 整机核数
const CPU_LIMIT_RATIO: f32 = 0.85;
const GAIN_LIMIT: f64 = 0.05;
const ERROR_LIMIT: f64 = 1.0;
const KEEP_ROWS: i64 = 10;

const RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";

/// 检测为全局单例：与压测任务注册表双向互斥
#[derive(Clone, Default)]
pub struct CapacityState {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl CapacityState {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 开始检测；已在运行或压测任务非空时返回可读原因
    pub fn try_begin(&self, stress_busy: bool) -> Result<Arc<AtomicBool>, String> {
        if stress_busy {
            return Err("有压测任务正在运行，检测会与压测互相干扰；请先停止全部压测任务".to_string());
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("本机能力检测已在运行，请稍候或先取消当前检测".to_string());
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(self.cancel.clone())
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// 取消检测，返回是否命中
    pub fn cancel(&self) -> bool {
        if !self.is_running() {
            return false;
        }
        self.cancel.store(true, Ordering::SeqCst);
        true
    }
}

/// 逐档计数器：首秒预热结束后清零，只统计后 2 秒
struct StageCounters {
    total: AtomicU64,
    errors: AtomicU64,
    sum_ms: AtomicU64,
    buckets: Vec<AtomicU64>,
}

impl StageCounters {
    fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            sum_ms: AtomicU64::new(0),
            buckets: (0..=10_000).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    fn reset(&self) {
        self.total.store(0, Ordering::SeqCst);
        self.errors.store(0, Ordering::SeqCst);
        self.sum_ms.store(0, Ordering::SeqCst);
        for bucket in &self.buckets {
            bucket.store(0, Ordering::SeqCst);
        }
    }

    fn record(&self, latency_ms: u64, ok: bool) {
        self.total.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        let index = latency_ms.min(10_000) as usize;
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
    }

    fn percentile(&self, p: f64) -> f64 {
        let total = self.total.load(Ordering::SeqCst);
        if total == 0 {
            return 0.0;
        }
        let target = (total as f64 * p).ceil() as u64;
        let mut acc = 0u64;
        for (index, bucket) in self.buckets.iter().enumerate() {
            acc += bucket.load(Ordering::Relaxed);
            if acc >= target {
                return index as f64;
            }
        }
        10_000.0
    }
}

/// AstroMan 进程资源采样：CPU 为占用核数，内存为 MB
struct ProcSampler {
    sys: System,
    pid: sysinfo::Pid,
}

impl ProcSampler {
    fn new() -> Self {
        let mut sys = System::new();
        let pid = sysinfo::Pid::from_u32(std::process::id());
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        Self { sys, pid }
    }

    fn sample(&mut self) -> (f32, u64) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        match self.sys.process(self.pid) {
            Some(process) => (process.cpu_usage() / 100.0, process.memory() / 1024 / 1024),
            None => (0.0, 0),
        }
    }
}

/// 回环 HTTP 服务：只返回固定 2 字节响应，支持 keep-alive
struct Loopback {
    addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Loopback {
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
            format!("无法启动本机检测服务（127.0.0.1 随机端口）：{error}")
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("设置检测服务非阻塞失败：{error}"))?;
        let addr = listener
            .local_addr()
            .map_err(|error| format!("读取检测服务端口失败：{error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let flag = flag.clone();
                        std::thread::spawn(move || serve_connection(stream, flag));
                    }
                    Err(ref error) if error.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self { addr, stop })
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/probe", self.addr.port())
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve_connection(mut stream: TcpStream, stop: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(count) => {
                buffer.extend_from_slice(&chunk[..count]);
                while let Some(position) = find_header_end(&buffer) {
                    buffer.drain(..position + 4);
                    if stream.write_all(RESPONSE).is_err() {
                        return;
                    }
                }
            }
            Err(ref error) if error.kind() == ErrorKind::WouldBlock => continue,
            Err(ref error)
                if error.kind() == ErrorKind::TimedOut || error.kind() == ErrorKind::Interrupted =>
            {
                continue
            }
            Err(_) => return,
        }
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|item| item == b"\r\n\r\n")
}

/// 单档压测 worker：在窗口内持续请求，只统计预热结束后的完成请求
async fn stage_worker(
    client: reqwest::Client,
    url: String,
    counters: Arc<StageCounters>,
    stage_end: Instant,
    measure_from: Instant,
) {
    while Instant::now() < stage_end {
        let started = Instant::now();
        let result = client.get(&url).send().await;
        let finished = Instant::now();
        if finished < measure_from {
            continue;
        }
        let latency = started.elapsed().as_millis() as u64;
        let ok = match result {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        };
        counters.record(latency, ok);
    }
}

async fn run_probe(
    app: &AppHandle,
    cancel: Arc<AtomicBool>,
    url: String,
) -> Result<Vec<CapacitySample>, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(2048)
        .tcp_nodelay(true)
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| format!("构建检测客户端失败：{error}"))?;
    let cores = std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(1)
        .max(1);
    let mut sampler = ProcSampler::new();
    let mut samples: Vec<CapacitySample> = Vec::new();
    let probe_start = Instant::now();
    let mut last_qps: Option<f64> = None;
    let mut low_gain_streak = 0u32;
    let mut cpu_hot_streak = 0u32;

    for (index, concurrent) in STAGES.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(samples);
        }
        if probe_start.elapsed() >= Duration::from_secs(MAX_PROBE_SEC) {
            break;
        }
        let counters = Arc::new(StageCounters::new());
        let stage_end = Instant::now() + Duration::from_secs(STAGE_SEC);
        let measure_from = Instant::now() + Duration::from_secs(WARMUP_SEC);
        let mut workers = tokio::task::JoinSet::new();
        for _ in 0..*concurrent {
            workers.spawn(stage_worker(
                client.clone(),
                url.clone(),
                counters.clone(),
                stage_end,
                measure_from,
            ));
        }
        // 预热结束即清零：后 2 秒才是有效统计窗口
        tokio::time::sleep(Duration::from_secs(WARMUP_SEC)).await;
        counters.reset();
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        while Instant::now() < stage_end {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            ticker.tick().await;
        }
        workers.abort_all();
        while workers.join_next().await.is_some() {}

        let measured_sec = STAGE_SEC.saturating_sub(WARMUP_SEC).max(1) as f64;
        let total = counters.total.load(Ordering::SeqCst);
        let errors = counters.errors.load(Ordering::SeqCst);
        if total == 0 {
            // 被测本机服务没有完成请求，后续档位没有意义
            break;
        }
        let qps = total as f64 / measured_sec;
        let (cpu, mem) = sampler.sample();
        let cpu_ratio = cpu / cores as f32;
        let error_rate = errors as f64 / total as f64 * 100.0;
        let sample = CapacitySample {
            concurrent: *concurrent,
            qps,
            avg_rt: counters.sum_ms.load(Ordering::SeqCst) as f64 / total as f64,
            p95_rt: counters.percentile(0.95),
            error_rate,
            cpu,
            mem_mb: mem,
        };
        let _ = app.emit(
            "capacity:progress",
            json!({
                "stage": index,
                "concurrent": sample.concurrent,
                "qps": sample.qps,
                "cpu": sample.cpu,
                "mem": sample.mem_mb,
                "done": false,
            }),
        );

        if let Some(previous) = last_qps {
            let gain = if previous > 0.0 {
                (qps - previous) / previous
            } else {
                0.0
            };
            if gain < GAIN_LIMIT {
                low_gain_streak += 1;
            } else {
                low_gain_streak = 0;
            }
        }
        if cpu_ratio >= CPU_LIMIT_RATIO {
            cpu_hot_streak += 1;
        } else {
            cpu_hot_streak = 0;
        }
        last_qps = Some(qps);
        samples.push(sample);

        if cancel.load(Ordering::SeqCst) {
            break;
        }
        // 饱和判定：QPS 连续两档增幅 <5%、CPU 连续两档 ≥85%、错误率 ≥1%
        if low_gain_streak >= 2 || cpu_hot_streak >= 2 || error_rate >= ERROR_LIMIT {
            break;
        }
    }

    let _ = app.emit(
        "capacity:progress",
        json!({
            "stage": samples.len(),
            "concurrent": samples.last().map(|item| item.concurrent).unwrap_or(0),
            "qps": samples.iter().map(|item| item.qps).fold(0.0, f64::max),
            "cpu": samples.iter().map(|item| item.cpu).fold(0.0, f32::max),
            "mem": samples.iter().map(|item| item.mem_mb).max().unwrap_or(0),
            "done": true,
        }),
    );
    Ok(samples)
}

/// 结果落库：只保留最近 10 次
fn save_result(pool: &DbPool, result: &CapacityResult) -> Result<(), String> {
    let conn = pool.get().map_err(err)?;
    let samples_json = serde_json::to_string(&result.samples).map_err(err)?;
    conn.execute(
        "INSERT INTO machine_capacity
            (probed_at, max_qps, saturation_concurrent, cpu_peak, mem_peak, cores, os, reached_limit, samples_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            result.probed_at,
            result.max_qps,
            result.saturation_concurrent,
            result.cpu_peak,
            result.mem_peak as i64,
            result.cores,
            result.os,
            if result.reached_limit { 1 } else { 0 },
            samples_json,
        ],
    )
    .map_err(err)?;
    conn.execute(
        "DELETE FROM machine_capacity WHERE id NOT IN (
            SELECT id FROM machine_capacity ORDER BY probed_at DESC, id DESC LIMIT ?1
        )",
        params![KEEP_ROWS],
    )
    .map_err(err)?;
    Ok(())
}

/// 读取最近一次检测结果
pub fn latest_result(conn: &rusqlite::Connection) -> Result<Option<CapacityResult>, String> {
    let row = conn
        .query_row(
            "SELECT probed_at, max_qps, saturation_concurrent, cpu_peak, mem_peak, cores, os,
                    reached_limit, samples_json
             FROM machine_capacity ORDER BY probed_at DESC, id DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()
        .map_err(err)?;
    let Some((
        probed_at,
        max_qps,
        saturation_concurrent,
        cpu_peak,
        mem_peak,
        cores,
        os,
        reached_limit,
        samples_json,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(CapacityResult {
        probed_at,
        max_qps,
        saturation_concurrent: saturation_concurrent.max(0) as u32,
        cpu_peak: cpu_peak as f32,
        mem_peak: mem_peak.max(0) as u64,
        cores: cores.max(0) as u32,
        os,
        reached_limit: reached_limit != 0,
        samples: serde_json::from_str::<Vec<CapacitySample>>(&samples_json).unwrap_or_default(),
    }))
}

/// 运行检测并保存结果；命令与前端只关心最终能力画像
#[tauri::command]
pub async fn capacity_probe(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<CapacityResult, String> {
    let stress_busy = !state.stress_jobs.is_empty();
    let cancel = state.capacity.try_begin(stress_busy)?;
    let pool = state.pool.clone();

    let outcome = async {
        let server = Loopback::start()?;
        let samples = run_probe(&app, cancel.clone(), server.url()).await?;
        if samples.is_empty() {
            return Err("检测没有采集到有效请求，请重试".to_string());
        }
        if cancel.load(Ordering::SeqCst) {
            return Err("检测已取消".to_string());
        }
        let cores = std::thread::available_parallelism()
            .map(|value| value.get() as u32)
            .unwrap_or(1)
            .max(1);
        let max_qps = samples.iter().map(|item| item.qps).fold(0.0, f64::max);
        let saturation = samples
            .iter()
            .max_by(|a, b| a.qps.partial_cmp(&b.qps).unwrap_or(std::cmp::Ordering::Equal))
            .map(|item| item.concurrent)
            .unwrap_or(0);
        let max_stage = STAGES[STAGES.len() - 1];
        // 达到 1024 且仍在增长/CPU 未打满时不算触顶，避免把下限当极限
        let reached_limit = saturation < max_stage
            || samples
                .last()
                .map(|item| item.cpu / cores as f32 >= CPU_LIMIT_RATIO || item.error_rate >= ERROR_LIMIT)
                .unwrap_or(false);
        let result = CapacityResult {
            probed_at: now_ms(),
            max_qps,
            saturation_concurrent: saturation,
            cpu_peak: samples.iter().map(|item| item.cpu).fold(0.0, f32::max),
            mem_peak: samples.iter().map(|item| item.mem_mb).max().unwrap_or(0),
            cores,
            os: format!(
                "{} {}",
                System::name().unwrap_or_default(),
                System::os_version().unwrap_or_default()
            )
            .trim()
            .to_string(),
            reached_limit,
            samples,
        };
        save_result(&pool, &result)?;
        Ok::<CapacityResult, String>(result)
    }
    .await;

    state.capacity.finish();
    outcome
}

/// 读取最近一次检测结果（互斥只限制启动，不影响读取）
#[tauri::command]
pub fn capacity_get(state: State<'_, AppState>) -> Result<Option<CapacityResult>, String> {
    let conn = state.pool.get().map_err(err)?;
    latest_result(&conn)
}

/// 取消正在运行的检测
#[tauri::command]
pub fn capacity_cancel(state: State<'_, AppState>) -> Result<(), String> {
    if state.capacity.cancel() {
        Ok(())
    } else {
        Err("当前没有运行中的能力检测".to_string())
    }
}
