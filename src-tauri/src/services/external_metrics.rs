//! 外部监控指标 CSV 导入（R11 / 3.6.1）。
//!
//! 约定：两列 `t,v`，`t` 为相对压测开始的秒数，也接受绝对时间戳
//! （>1e9 时按最小值归一，毫秒级时间戳先除以 1000）。文件名即数据来源。
//! 解析放在 Rust，报告落库在同一侧完成，前端只负责选文件与填元信息。

use crate::models::{ExternalMetricPoint, ExternalMetricSeries};
use std::collections::BTreeMap;
use std::path::Path;

/// 单份报告最多保留的序列数；导入第 11 条时明确报错
pub const MAX_SERIES: usize = 10;
/// 单条序列最多采样点；超出按时间正序截断
pub const MAX_POINTS: usize = 3600;
/// CSV 体积上限，避免误选超大文件把内存吃满
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_METRIC_CHARS: usize = 64;
const ABSOLUTE_SECONDS: f64 = 1e9;
const ABSOLUTE_MILLIS: f64 = 1e12;

pub const UNITS: [&str; 4] = ["percent", "ms", "count", "bytes"];

/// 解析一份 CSV 为一条序列；同一 `source + metric` 的旧数据由调用方替换。
pub fn parse_csv(path: &Path, metric: &str, unit: &str) -> Result<ExternalMetricSeries, String> {
    let metric = metric.trim();
    if metric.is_empty() {
        return Err("E_EXTERNAL_METRIC: 指标名不能为空".into());
    }
    if metric.chars().count() > MAX_METRIC_CHARS {
        return Err(format!(
            "E_EXTERNAL_METRIC: 指标名最长 {MAX_METRIC_CHARS} 个字符"
        ));
    }
    let unit = unit.trim().to_ascii_lowercase();
    if !UNITS.contains(&unit.as_str()) {
        return Err(format!("E_EXTERNAL_UNIT: 单位只支持 {}", UNITS.join(" / ")));
    }

    let size = std::fs::metadata(path)
        .map_err(|error| format!("E_EXTERNAL_READ: 无法读取文件 {error}"))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "E_EXTERNAL_LARGE: CSV 超过 {} MB，请先在监控侧降采样",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| format!("E_EXTERNAL_READ: {error}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');

    let mut raw: Vec<(f64, f64)> = Vec::new();
    let mut header_checked = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split([',', ';', '\t']).map(unquote).collect();
        if fields.len() < 2 {
            return Err(format!("E_EXTERNAL_CSV: 第 {} 行需要 t,v 两列", index + 1));
        }
        let t = fields[0].trim().parse::<f64>();
        let v = fields[1].trim().parse::<f64>();
        let (t, v) = match (t, v) {
            (Ok(t), Ok(v)) => (t, v),
            _ => {
                // 首个有效行解析失败时按表头（t,v）跳过，其余行一律报错
                if !header_checked {
                    header_checked = true;
                    continue;
                }
                return Err(format!(
                    "E_EXTERNAL_CSV: 第 {} 行无法解析为数字：{}",
                    index + 1,
                    line
                ));
            }
        };
        header_checked = true;
        if !t.is_finite() || !v.is_finite() {
            return Err(format!("E_EXTERNAL_CSV: 第 {} 行包含 NaN/Inf", index + 1));
        }
        raw.push((t, v));
    }

    if raw.is_empty() {
        return Err("E_EXTERNAL_EMPTY: CSV 里没有可用采样点".into());
    }

    // 绝对时间戳：毫秒先归到秒，再整体减去最小值，得到相对秒数
    let absolute = raw.iter().any(|(t, _)| *t > ABSOLUTE_SECONDS);
    let scale = if raw.iter().any(|(t, _)| *t >= ABSOLUTE_MILLIS) {
        1000.0
    } else {
        1.0
    };
    let base = if absolute {
        raw.iter()
            .map(|(t, _)| *t / scale)
            .fold(f64::INFINITY, f64::min)
    } else {
        0.0
    };

    // 同一秒出现多次时保留最后一条；BTreeMap 顺带完成时间正序
    let mut points: BTreeMap<i64, f64> = BTreeMap::new();
    for (t, v) in raw {
        let seconds = if absolute { t / scale - base } else { t };
        points.insert(seconds.max(0.0).round() as i64, v);
    }

    let truncated = points.len() > MAX_POINTS;
    let points: Vec<ExternalMetricPoint> = points
        .into_iter()
        .take(MAX_POINTS)
        .map(|(t, v)| ExternalMetricPoint { t, v })
        .collect();
    if points.is_empty() {
        return Err("E_EXTERNAL_EMPTY: CSV 里没有可用采样点".into());
    }
    if truncated {
        log::warn!("外部指标 {metric} 超过 {MAX_POINTS} 点，已按时间正序截断");
    }

    let file_name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("import")
        .to_string();
    Ok(ExternalMetricSeries {
        source: format!("csv:{file_name}"),
        metric: metric.to_string(),
        unit,
        points,
    })
}

/// 合并一条序列：同 source + metric 原位覆盖，总数超过上限时报错。
pub fn merge_series(
    mut existing: Vec<ExternalMetricSeries>,
    incoming: ExternalMetricSeries,
) -> Result<Vec<ExternalMetricSeries>, String> {
    if let Some(index) = existing
        .iter()
        .position(|item| item.source == incoming.source && item.metric == incoming.metric)
    {
        existing[index] = incoming;
        return Ok(existing);
    }
    if existing.len() >= MAX_SERIES {
        return Err(format!(
            "E_EXTERNAL_LIMIT: 单份报告最多 {MAX_SERIES} 条外部指标，请先删除或复用已有指标"
        ));
    }
    existing.push(incoming);
    Ok(existing)
}

fn unquote(field: &str) -> &str {
    field.trim().trim_matches('"').trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn write_csv(body: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "astroman-metrics-{}-{}.csv",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(body).unwrap();
        path
    }

    #[test]
    fn parses_bom_blank_lines_and_header() {
        let path = write_csv(b"\xef\xbb\xbft,v\r\n\r\n0,12.5\r\n1,13.1\r\n# comment\r\n");
        let series = parse_csv(&path, "cpu.usage", "percent").unwrap();
        assert_eq!(series.metric, "cpu.usage");
        assert_eq!(series.unit, "percent");
        assert_eq!(
            series.points,
            vec![
                ExternalMetricPoint { t: 0, v: 12.5 },
                ExternalMetricPoint { t: 1, v: 13.1 }
            ]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sorts_unsorted_rows_and_keeps_last_duplicate() {
        let path = write_csv(b"3,30\n1,10\n2,20\n2,25\n");
        let series = parse_csv(&path, "gc.pause", "ms").unwrap();
        assert_eq!(series.points.len(), 3);
        assert_eq!(series.points[0], ExternalMetricPoint { t: 1, v: 10.0 });
        assert_eq!(series.points[1], ExternalMetricPoint { t: 2, v: 25.0 });
        assert_eq!(series.points[2], ExternalMetricPoint { t: 3, v: 30.0 });
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn normalizes_absolute_timestamps() {
        let path = write_csv(b"1700000000,1\n1700000005,2\n1700000010,3\n");
        let series = parse_csv(&path, "db.pool.wait", "ms").unwrap();
        assert_eq!(
            series.points.iter().map(|p| p.t).collect::<Vec<_>>(),
            vec![0, 5, 10]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn normalizes_absolute_millis() {
        let path = write_csv(b"1700000000000,1\n1700000002500,2\n");
        let series = parse_csv(&path, "cpu.usage", "percent").unwrap();
        assert_eq!(
            series.points.iter().map(|p| p.t).collect::<Vec<_>>(),
            vec![0, 3]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn truncates_to_point_limit() {
        let mut body = String::new();
        for index in 0..(MAX_POINTS + 50) {
            body.push_str(&format!("{index},{}\n", index as f64));
        }
        let path = write_csv(body.as_bytes());
        let series = parse_csv(&path, "thread.pool.active", "count").unwrap();
        assert_eq!(series.points.len(), MAX_POINTS);
        assert_eq!(series.points[0].t, 0);
        assert_eq!(series.points[MAX_POINTS - 1].t, (MAX_POINTS - 1) as i64);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_bad_rows_and_units() {
        let bad = write_csv(b"0,1\nfoo,2\n");
        assert!(parse_csv(&bad, "cpu.usage", "percent")
            .unwrap_err()
            .starts_with("E_EXTERNAL_CSV"));
        let empty = write_csv(b"t,v\n");
        assert!(parse_csv(&empty, "cpu.usage", "percent")
            .unwrap_err()
            .starts_with("E_EXTERNAL_EMPTY"));
        let good = write_csv(b"0,1\n");
        assert!(parse_csv(&good, "cpu.usage", "furlong")
            .unwrap_err()
            .starts_with("E_EXTERNAL_UNIT"));
        for path in [bad, empty, good] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn merge_replaces_same_metric_and_caps_series() {
        let mut existing = Vec::new();
        for index in 0..MAX_SERIES {
            existing.push(ExternalMetricSeries {
                source: format!("csv:s{index}"),
                metric: format!("m{index}"),
                unit: "count".into(),
                points: vec![ExternalMetricPoint { t: 0, v: 1.0 }],
            });
        }
        let replaced = ExternalMetricSeries {
            source: "csv:s0".into(),
            metric: "m0".into(),
            unit: "count".into(),
            points: vec![ExternalMetricPoint { t: 9, v: 9.0 }],
        };
        let merged = merge_series(existing.clone(), replaced).unwrap();
        assert_eq!(merged.len(), MAX_SERIES);
        assert_eq!(merged[0].points[0].t, 9);

        let extra = ExternalMetricSeries {
            source: "csv:new".into(),
            metric: "new".into(),
            unit: "count".into(),
            points: vec![ExternalMetricPoint { t: 0, v: 1.0 }],
        };
        assert!(merge_series(existing, extra)
            .unwrap_err()
            .starts_with("E_EXTERNAL_LIMIT"));
    }
}
