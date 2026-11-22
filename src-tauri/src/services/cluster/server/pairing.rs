//! 配对与时钟：配对码生成、失败限速、时钟偏移计算

use super::*;

pub(crate) fn clock_metrics(t0: i64, t1: i64, t2: i64) -> (f64, f64) {
    let rtt = (t2 - t0).max(0) as f64;
    let offset = t1 as f64 - (t0 as f64 + t2 as f64) / 2.0;
    (offset, rtt / 2.0)
}

pub(super) fn new_pair_code() -> String {
    let value = Uuid::new_v4().as_u128() % 10u128.pow(PAIR_CODE_DIGITS as u32);
    format!("{value:0width$}", width = PAIR_CODE_DIGITS)
}

/// 记录一次配对码错误并返回给从节点的提示
///
/// - 同一来源 `PAIR_FAIL_WINDOW_MS` 内失败超过 `PAIR_FAIL_PER_WINDOW` 次：报"过于频繁"；
/// - 当前码累计失败达到 `PAIR_FAIL_TOTAL`：立即作废并换新码，让暴力枚举拿不到稳定目标。
pub(super) fn register_pair_failure(
    state: &mut SessionState,
    ip: &str,
    now: i64,
    ttl_sec: i64,
) -> String {
    let window_hit = {
        let entry = state
            .pair_failures
            .entry(ip.to_string())
            .or_insert((0, now));
        if now - entry.1 > PAIR_FAIL_WINDOW_MS {
            *entry = (0, now);
        }
        entry.0 += 1;
        entry.0 >= PAIR_FAIL_PER_WINDOW
    };
    // 只保留最近一分钟的记录，长期运行不会无限增长
    state.pair_failures.retain(|_, (_, at)| now - *at <= 60_000);
    state.pair_fail_total += 1;
    if state.pair_fail_total >= PAIR_FAIL_TOTAL {
        let previous = state
            .pair_code
            .as_ref()
            .map(|code| code.code.clone())
            .unwrap_or_default();
        let mut candidate = new_pair_code();
        while candidate == previous {
            candidate = new_pair_code();
        }
        state.pair_code = Some(PairCode {
            code: candidate,
            expires_at: now + ttl_sec * 1000,
        });
        state.pair_fail_total = 0;
        state.pair_failures.clear();
        return "配对失败次数过多，配对码已刷新，请查看主节点上的最新配对码".to_string();
    }
    if window_hit {
        "配对尝试过于频繁，请稍后再试".to_string()
    } else {
        "配对码不正确".to_string()
    }
}
