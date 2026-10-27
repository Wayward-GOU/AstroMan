use crate::models::{
    AppInfo, CodeBucket, DailyPoint, DashboardMetrics, HourPoint, ProjectPoint, ReportPoint,
    TopRequest, WorkspaceStats,
};
use crate::state::AppState;
use crate::util::{err, now_ms};
use chrono::{Duration, Local, TimeZone};
use rusqlite::params;
use tauri::State;

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        db_path: state.db_path.to_string_lossy().to_string(),
    }
}

/// 工作台首屏所需的聚合统计
#[tauri::command]
pub fn workspace_stats(state: State<'_, AppState>) -> Result<WorkspaceStats, String> {
    let conn = state.pool.get().map_err(err)?;
    let count = |sql: &str| -> Result<i64, String> {
        conn.query_row(sql, [], |row| row.get(0)).map_err(err)
    };
    let week_ago = now_ms() - 7 * 24 * 3600 * 1000;

    Ok(WorkspaceStats {
        project_count: count("SELECT COUNT(*) FROM projects")?,
        collection_count: count("SELECT COUNT(*) FROM collections")?,
        request_count: count("SELECT COUNT(*) FROM requests")?,
        environment_count: count("SELECT COUNT(*) FROM environments")?,
        history_count: count("SELECT COUNT(*) FROM request_history")?,
        report_count: count("SELECT COUNT(*) FROM test_reports")?,
        report_request_total: count("SELECT COALESCE(SUM(total_requests), 0) FROM test_reports")?,
        report_success_total: count("SELECT COALESCE(SUM(success_count), 0) FROM test_reports")?,
        recent_history: conn
            .query_row(
                "SELECT COUNT(*) FROM request_history WHERE created_at >= ?1",
                params![week_ago],
                |row| row.get(0),
            )
            .map_err(err)?,
        recent_reports: conn
            .query_row(
                "SELECT COUNT(*) FROM test_reports WHERE created_at >= ?1",
                params![week_ago],
                |row| row.get(0),
            )
            .map_err(err)?,
    })
}

/// 工作台数据大屏：近 14 天趋势、状态码分布、高频接口与压测走势
#[tauri::command]
pub fn dashboard_metrics(state: State<'_, AppState>) -> Result<DashboardMetrics, String> {
    let conn = state.pool.get().map_err(err)?;

    let today = Local::now().date_naive();
    let start = today - Duration::days(13);
    let start_ms = Local
        .from_local_datetime(&start.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0);

    // 按本地日期分桶，缺失的日期在下面补 0
    let mut by_day = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT date(created_at / 1000, 'unixepoch', 'localtime') AS day,
                        COUNT(*),
                        COALESCE(SUM(CASE WHEN status_code BETWEEN 200 AND 399 THEN 1 ELSE 0 END), 0),
                        COALESCE(AVG(duration_ms), 0)
                 FROM request_history
                 WHERE created_at >= ?1
                 GROUP BY day",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![start_ms], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(err)?;
        for row in rows.filter_map(Result::ok) {
            by_day.insert(row.0, (row.1, row.2, row.3));
        }
    }

    let days: Vec<DailyPoint> = (0..14)
        .map(|i| {
            let key = (start + Duration::days(i))
                .format("%Y-%m-%d")
                .to_string();
            let (total, success, avg_rt) = by_day.get(&key).copied().unwrap_or((0, 0, 0.0));
            DailyPoint {
                date: key,
                total,
                success,
                avg_rt,
            }
        })
        .collect();

    let (s2, s3, s4, s5, other): (i64, i64, i64, i64, i64) = conn
        .query_row(
            "SELECT
               COALESCE(SUM(CASE WHEN status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN status_code BETWEEN 300 AND 399 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN status_code BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN status_code BETWEEN 500 AND 599 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE
                   WHEN status_code IS NULL OR status_code < 200 OR status_code >= 600 THEN 1
                   ELSE 0 END), 0)
             FROM request_history",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(err)?;

    let code_buckets = vec![
        CodeBucket {
            label: "2xx".to_string(),
            count: s2,
        },
        CodeBucket {
            label: "3xx".to_string(),
            count: s3,
        },
        CodeBucket {
            label: "4xx".to_string(),
            count: s4,
        },
        CodeBucket {
            label: "5xx".to_string(),
            count: s5,
        },
        CodeBucket {
            label: "异常".to_string(),
            count: other,
        },
    ];

    let top_requests = {
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(NULLIF(TRIM(request_name), ''), url),
                        COALESCE(method, 'GET'),
                        url,
                        COUNT(*),
                        COALESCE(AVG(duration_ms), 0),
                        COALESCE(SUM(CASE WHEN status_code BETWEEN 200 AND 399 THEN 1 ELSE 0 END), 0)
                 FROM request_history
                 GROUP BY request_name, method, url
                 ORDER BY COUNT(*) DESC
                 LIMIT 5",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                let count: i64 = row.get(3)?;
                let success: i64 = row.get(5)?;
                Ok(TopRequest {
                    name: row.get(0)?,
                    method: row.get(1)?,
                    url: row.get(2)?,
                    count,
                    avg_rt: row.get(4)?,
                    success_rate: if count > 0 {
                        success as f64 * 100.0 / count as f64
                    } else {
                        0.0
                    },
                })
            })
            .map_err(err)?;
        rows.filter_map(Result::ok).collect::<Vec<_>>()
    };

    let mut report_points = {
        let mut stmt = conn
            .prepare(
                "SELECT id, plan_name, p95_rt, error_rate, total_requests, created_at
                 FROM test_reports
                 ORDER BY created_at DESC
                 LIMIT 6",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ReportPoint {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    p95_rt: row.get(2)?,
                    error_rate: row.get(3)?,
                    total_requests: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(err)?;
        rows.filter_map(Result::ok).collect::<Vec<_>>()
    };
    report_points.reverse();

    // 请求方法分布：固定顺序，未知方法归入「其他」
    let method_buckets = {
        let (get, post, put, delete, patch, other): (i64, i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT
                   COALESCE(SUM(CASE WHEN UPPER(COALESCE(method, 'GET')) = 'GET' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN UPPER(COALESCE(method, '')) = 'POST' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN UPPER(COALESCE(method, '')) = 'PUT' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN UPPER(COALESCE(method, '')) = 'DELETE' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN UPPER(COALESCE(method, '')) = 'PATCH' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN UPPER(COALESCE(method, 'GET'))
                     NOT IN ('GET', 'POST', 'PUT', 'DELETE', 'PATCH') THEN 1 ELSE 0 END), 0)
                 FROM request_history",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .map_err(err)?;
        vec![
            ("GET", get),
            ("POST", post),
            ("PUT", put),
            ("DELETE", delete),
            ("PATCH", patch),
            ("其他", other),
        ]
        .into_iter()
        .map(|(label, count)| CodeBucket {
            label: label.to_string(),
            count,
        })
        .collect::<Vec<_>>()
    };

    // 今日各时段请求量：按本地小时分桶，缺失时段补 0
    let hour_buckets = {
        let today_start = Local
            .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);
        let mut by_hour = std::collections::HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT CAST(strftime('%H', created_at / 1000, 'unixepoch', 'localtime') AS INTEGER),
                        COUNT(*)
                 FROM request_history
                 WHERE created_at >= ?1
                 GROUP BY 1",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![today_start], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(err)?;
        for (hour, count) in rows.filter_map(Result::ok) {
            by_hour.insert(hour, count);
        }
        (0..24)
            .map(|hour| HourPoint {
                hour,
                count: by_hour.get(&hour).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>()
    };

    // 响应耗时分布：按固定区间统计，颜色从快到慢递进
    let latency_buckets = {
        let (fast, quick, normal, slow, very_slow): (i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT
                   COALESCE(SUM(CASE WHEN COALESCE(duration_ms, 0) < 100 THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN COALESCE(duration_ms, 0) >= 100
                     AND COALESCE(duration_ms, 0) < 300 THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN COALESCE(duration_ms, 0) >= 300
                     AND COALESCE(duration_ms, 0) < 1000 THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN COALESCE(duration_ms, 0) >= 1000
                     AND COALESCE(duration_ms, 0) < 3000 THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN COALESCE(duration_ms, 0) >= 3000 THEN 1 ELSE 0 END), 0)
                 FROM request_history",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(err)?;
        vec![
            ("<100ms", fast),
            ("100-300ms", quick),
            ("300ms-1s", normal),
            ("1-3s", slow),
            ("≥3s", very_slow),
        ]
        .into_iter()
        .map(|(label, count)| CodeBucket {
            label: label.to_string(),
            count,
        })
        .collect::<Vec<_>>()
    };

    // 项目维度调用量：请求可能已删，归入「未归类」
    let top_projects = {
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(p.name, '未归类') AS project_name,
                        COUNT(*),
                        COALESCE(AVG(h.duration_ms), 0),
                        COALESCE(SUM(CASE WHEN h.status_code BETWEEN 200 AND 399 THEN 1 ELSE 0 END), 0)
                 FROM request_history h
                 LEFT JOIN requests r ON r.id = h.request_id
                 LEFT JOIN collections c ON c.id = r.collection_id
                 LEFT JOIN projects p ON p.id = c.project_id
                 GROUP BY project_name
                 ORDER BY COUNT(*) DESC
                 LIMIT 5",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                let count: i64 = row.get(1)?;
                let success: i64 = row.get(3)?;
                Ok(ProjectPoint {
                    name: row.get(0)?,
                    count,
                    avg_rt: row.get(2)?,
                    success_rate: if count > 0 {
                        success as f64 * 100.0 / count as f64
                    } else {
                        0.0
                    },
                })
            })
            .map_err(err)?;
        rows.filter_map(Result::ok).collect::<Vec<_>>()
    };

    Ok(DashboardMetrics {
        days,
        code_buckets,
        top_requests,
        report_points,
        method_buckets,
        hour_buckets,
        latency_buckets,
        top_projects,
    })
}
