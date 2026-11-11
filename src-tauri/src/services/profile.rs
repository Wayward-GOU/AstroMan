//! 运行档位与抓包预算（P-05）。
//!
//! 4GB 办公机和 16GB 开发机不能共用一套 128MB body 预算：
//! 启动时按总内存定档一次，避免运行中反复切换丢流量。
//! 用户可在设置页选择「自动 / 低内存 / 标准」，保存后立即应用新的抓包预算。

use serde::{Deserialize, Serialize};
use sysinfo::System;

/// 抓包与响应体预算
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureLimits {
    /// 内存里最多保留的流量条数
    pub max_flows: usize,
    /// 流量 body 总预算
    pub max_body_bytes: u64,
    /// 单条响应正文入内存上限
    pub max_response_bytes: usize,
    /// 低内存模式：前端据此关闭图片预览、减少自动格式化
    pub low_memory: bool,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self::standard()
    }
}

impl CaptureLimits {
    pub const fn low() -> Self {
        Self {
            max_flows: 2_000,
            max_body_bytes: 32 * 1024 * 1024,
            max_response_bytes: 2 * 1024 * 1024,
            low_memory: true,
        }
    }

    pub const fn standard() -> Self {
        Self {
            max_flows: 5_000,
            max_body_bytes: 96 * 1024 * 1024,
            max_response_bytes: 5 * 1024 * 1024,
            low_memory: false,
        }
    }

    pub const fn high() -> Self {
        Self {
            max_flows: 5_000,
            max_body_bytes: 128 * 1024 * 1024,
            max_response_bytes: 5 * 1024 * 1024,
            low_memory: false,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{} 条 / body 预算 {}MB / 单响应 {}MB",
            self.max_flows,
            self.max_body_bytes / 1024 / 1024,
            self.max_response_bytes / 1024 / 1024
        )
    }
}

/// 当前档位。`Auto` 只用于设置值，解析后一定会落到具体档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PerfTier {
    Low,
    Standard,
    High,
}

/// 读取本机总内存；读取失败时不猜，按标准档处理。
pub fn total_memory_bytes() -> Option<u64> {
    let mut system = System::new();
    system.refresh_memory();
    let total = system.total_memory();
    (total > 0).then_some(total)
}

/// 设置值归一化：只接受 low / standard，其余一律 auto。
pub fn normalize_mode(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "standard" => "standard",
        _ => "auto",
    }
}

/// 按设置值与总内存解析档位。
///
/// - `low` / `standard` 是用户显式选择，不再随内存变化；
/// - `auto`（含旧配置的空串与未知值）按总内存分档：≤6GB 低内存、≥16GB 高档、其余标准。
pub fn resolve(mode: &str, total_memory: Option<u64>) -> (PerfTier, CaptureLimits) {
    match mode.trim().to_ascii_lowercase().as_str() {
        "low" => (PerfTier::Low, CaptureLimits::low()),
        "standard" => (PerfTier::Standard, CaptureLimits::standard()),
        _ => {
            let total = total_memory.unwrap_or(8 * 1024 * 1024 * 1024);
            if total <= 6 * 1024 * 1024 * 1024 {
                (PerfTier::Low, CaptureLimits::low())
            } else if total >= 16 * 1024 * 1024 * 1024 {
                (PerfTier::High, CaptureLimits::high())
            } else {
                (PerfTier::Standard, CaptureLimits::standard())
            }
        }
    }
}

/// 启动后固定的运行档位，供设置页与抓包预算读取。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProfile {
    pub tier: PerfTier,
    pub limits: CaptureLimits,
    pub total_memory_bytes: Option<u64>,
}

/// 读取内存并生成当前运行档位。
pub fn active(mode: &str) -> ActiveProfile {
    let total = total_memory_bytes();
    let (tier, limits) = resolve(mode, total);
    ActiveProfile {
        tier,
        limits,
        total_memory_bytes: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_modes_ignore_memory() {
        assert_eq!(
            resolve("low", Some(64 * 1024 * 1024 * 1024)).0,
            PerfTier::Low
        );
        assert_eq!(
            resolve("standard", Some(4 * 1024 * 1024 * 1024)).0,
            PerfTier::Standard
        );
    }

    #[test]
    fn auto_follows_total_memory() {
        assert_eq!(
            resolve("auto", Some(4 * 1024 * 1024 * 1024)).0,
            PerfTier::Low
        );
        assert_eq!(
            resolve("auto", Some(8 * 1024 * 1024 * 1024)).0,
            PerfTier::Standard
        );
        assert_eq!(
            resolve("auto", Some(32 * 1024 * 1024 * 1024)).0,
            PerfTier::High
        );
    }

    #[test]
    fn empty_or_unknown_mode_falls_back_to_auto() {
        assert_eq!(resolve("", Some(4 * 1024 * 1024 * 1024)).0, PerfTier::Low);
        assert_eq!(
            resolve("turbo", Some(8 * 1024 * 1024 * 1024)).0,
            PerfTier::Standard
        );
    }

    #[test]
    fn low_memory_budget_is_smaller() {
        let low = CaptureLimits::low();
        let standard = CaptureLimits::standard();
        assert!(low.max_flows < standard.max_flows);
        assert!(low.max_body_bytes < standard.max_body_bytes);
        assert!(low.max_response_bytes < standard.max_response_bytes);
        assert!(low.low_memory);
    }
}
