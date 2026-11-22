//! 池 Excel 模板与导入（v20）
//!
//! 模板 sheet「数据」只写表头并整列设为文本，避免手机号等长数字被 Excel
//! 转成科学计数法；导入时按池列名映射整行数据，首列必填。

use std::collections::{HashMap, HashSet};

use calamine::{open_workbook_auto, Data, Reader};
use rusqlite::Connection;
use rust_xlsxwriter::{Format, Workbook};
use serde::Serialize;

use crate::services::pool::{self, PoolImportRow};

/// 单次导入行数上限
pub const MAX_EXCEL_ROWS: usize = 50_000;

/// 写入 Excel 导入模板
pub fn write_template(
    path: &str,
    pool_name: &str,
    columns: &[String],
    sensitive: bool,
) -> Result<(), String> {
    if columns.is_empty() {
        return Err("池没有可导出的列".to_string());
    }
    let mut workbook = Workbook::new();
    let header_format = Format::new().set_bold().set_num_format("@");
    let text_format = Format::new().set_num_format("@");
    {
        let sheet = workbook.add_worksheet();
        sheet
            .set_name("数据")
            .map_err(|error| format!("写入工作表失败：{error}"))?;
        for (index, column) in columns.iter().enumerate() {
            let col = u16::try_from(index).map_err(|_| "列数超出 Excel 上限".to_string())?;
            sheet
                .write_string_with_format(0, col, column, &header_format)
                .map_err(|error| format!("写入表头失败：{error}"))?;
            sheet
                .set_column_width(col, 18)
                .map_err(|error| format!("设置列宽失败：{error}"))?;
            sheet
                .set_column_format(col, &text_format)
                .map_err(|error| format!("设置列格式失败：{error}"))?;
        }
        sheet
            .set_freeze_panes(1, 0)
            .map_err(|error| format!("冻结首行失败：{error}"))?;
    }
    {
        let notes = workbook.add_worksheet();
        notes
            .set_name("说明")
            .map_err(|error| format!("写入工作表失败：{error}"))?;
        let mut lines = vec![
            format!("池名：{pool_name}"),
            format!("表头：{}", columns.join(",")),
            "填写规则：".to_string(),
            "1. 「数据」工作表第一行是表头，必须与池列名完全一致，不要修改表头。".to_string(),
            "2. 首列必填；首列为空的行会被跳过。".to_string(),
            format!("3. 单次最多导入 {MAX_EXCEL_ROWS} 行。"),
            format!(
                "4. 敏感池导入后默认脱敏展示：{}。",
                if sensitive { "是" } else { "否" }
            ),
            "5. 建议保持整列文本格式，避免手机号或长数字被转为科学计数法。".to_string(),
        ];
        lines.push("6. 列名只能包含字母、数字、下划线，且不能以数字开头。".to_string());
        for (index, line) in lines.iter().enumerate() {
            let row = u32::try_from(index).map_err(|_| "说明行数超出 Excel 上限".to_string())?;
            notes
                .write_string(row, 0, line)
                .map_err(|error| format!("写入说明失败：{error}"))?;
        }
        notes
            .set_column_width(0, 90)
            .map_err(|error| format!("设置列宽失败：{error}"))?;
    }
    workbook
        .save(path)
        .map_err(|error| format!("保存 Excel 模板失败：{error}"))?;
    Ok(())
}

/// Excel 导入结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelImportResult {
    pub inserted: usize,
    pub skipped: usize,
    /// 表头存在但池里没有的列
    pub ignored_columns: Vec<String>,
    /// 长数字告警（建议改用文本格式）
    pub numeric_warnings: Vec<String>,
}

/// 单元格转字符串：整数浮点去掉 `.0`，布尔转 true/false
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(text) => text.clone(),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => {
            if value.fract() == 0.0 && value.abs() < 9.2e18 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            }
        }
        Data::Bool(flag) => flag.to_string(),
        Data::DateTime(value) => value.to_string(),
        Data::DateTimeIso(value) | Data::DurationIso(value) => value.clone(),
        Data::Error(error) => error.to_string(),
    }
}

/// 数值且整数位超过 15 位（可能丢失精度）
fn is_large_numeric(cell: &Data) -> bool {
    match cell {
        Data::Int(value) => value.unsigned_abs() >= 10_u64.pow(15),
        Data::Float(value) => value.fract() == 0.0 && value.abs() >= 1e15,
        _ => false,
    }
}

/// 读取 Excel 并导入到指定池
pub fn import_excel(
    conn: &Connection,
    pool_id: i64,
    path: &str,
) -> Result<ExcelImportResult, String> {
    let columns = pool::pool_columns_by_id(conn, pool_id)?;
    let first_column = columns
        .first()
        .cloned()
        .ok_or_else(|| "池没有可导入的列".to_string())?;
    let mut workbook =
        open_workbook_auto(path).map_err(|error| format!("打开 Excel 失败：{error}"))?;
    let sheet_names = workbook.sheet_names();
    let sheet_name = sheet_names
        .first()
        .ok_or_else(|| "Excel 中没有工作表".to_string())?;
    let range = workbook
        .worksheet_range(sheet_name)
        .map_err(|error| format!("读取工作表失败：{error}"))?;

    let mut header_map: HashMap<String, usize> = HashMap::new();
    for index in 0..range.width() {
        let text = range
            .get((0, index))
            .map(cell_to_string)
            .unwrap_or_default();
        let name = text.trim().to_string();
        if !name.is_empty() {
            header_map.entry(name).or_insert(index);
        }
    }
    if !header_map.contains_key(&first_column) {
        let missing: Vec<String> = columns
            .iter()
            .filter(|column| !header_map.contains_key(*column))
            .cloned()
            .collect();
        return Err(format!(
            "Excel 缺少必需列：{}；请使用模板表头（{}）",
            missing.join("、"),
            columns.join(",")
        ));
    }
    let mut ignored_columns: Vec<String> = Vec::new();
    for index in 0..range.width() {
        let text = range
            .get((0, index))
            .map(cell_to_string)
            .unwrap_or_default();
        let name = text.trim().to_string();
        if name.is_empty() || columns.iter().any(|column| column == &name) {
            continue;
        }
        if !ignored_columns.iter().any(|column| column == &name) {
            ignored_columns.push(name);
        }
    }

    let mut rows: Vec<PoolImportRow> = Vec::new();
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut warned: HashSet<String> = HashSet::new();
    let mut numeric_warnings: Vec<String> = Vec::new();
    for row_index in 1..range.height() {
        let all_empty = (0..range.width()).all(|column_index| {
            range
                .get((row_index, column_index))
                .map(|cell| cell_to_string(cell).trim().is_empty())
                .unwrap_or(true)
        });
        if all_empty {
            continue;
        }
        processed += 1;
        if processed > MAX_EXCEL_ROWS {
            return Err(format!("单次最多导入 {MAX_EXCEL_ROWS} 行，请拆分文件后重试"));
        }
        let first_index = *header_map.get(&first_column).unwrap_or(&0);
        let first_value = range
            .get((row_index, first_index))
            .map(cell_to_string)
            .unwrap_or_default();
        if first_value.trim().is_empty() {
            skipped += 1;
            continue;
        }
        for column in &columns {
            let Some(index) = header_map.get(column) else {
                continue;
            };
            if let Some(cell) = range.get((row_index, *index)) {
                if is_large_numeric(cell) && warned.insert(column.clone()) {
                    numeric_warnings.push(format!(
                        "列「{column}」存在超过 15 位的数值，可能丢失精度，请将该列设置为文本格式"
                    ));
                }
            }
        }
        let mut data = HashMap::new();
        for column in &columns {
            let text = header_map
                .get(column)
                .and_then(|index| range.get((row_index, *index)))
                .map(cell_to_string)
                .unwrap_or_default();
            data.insert(column.clone(), text);
        }
        rows.push(PoolImportRow { data, weight: 1.0 });
    }

    let inserted = pool::insert_rows(conn, pool_id, &columns, &rows)?;
    Ok(ExcelImportResult {
        inserted,
        skipped,
        ignored_columns,
        numeric_warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn setup_db(columns: &[&str]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE data_pools (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'token',
                sensitive INTEGER NOT NULL DEFAULT 1,
                description TEXT,
                refresh_mode TEXT NOT NULL DEFAULT 'none',
                refresh_json TEXT NOT NULL DEFAULT '{}',
                columns_json TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE data_pool_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pool_id INTEGER NOT NULL,
                value TEXT NOT NULL,
                account TEXT,
                weight REAL NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL DEFAULT 0,
                cooldown_until INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                data_json TEXT NOT NULL DEFAULT '{}'
            );",
        )
        .unwrap();
        let raw = serde_json::to_string(columns).unwrap();
        conn.execute(
            "INSERT INTO data_pools (name, kind, sensitive, columns_json, created_at, updated_at)
             VALUES ('tokens', 'token', 1, ?1, 1, 1)",
            params![raw],
        )
        .unwrap();
        conn
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "astroman-pool-excel-{tag}-{}-{stamp}.xlsx",
            std::process::id()
        ))
    }

    #[test]
    fn template_roundtrip_imports_two_rows() {
        let conn = setup_db(&["token", "account"]);
        let columns = vec!["token".to_string(), "account".to_string()];
        let template = temp_path("template");
        let template_path = template.to_str().unwrap().to_string();
        write_template(&template_path, "tokens", &columns, true).unwrap();
        // 模板只有表头，导入 0 行
        let empty = import_excel(&conn, 1, &template_path).unwrap();
        assert_eq!(empty.inserted, 0);
        assert!(empty.ignored_columns.is_empty());

        // 按同一表头填两行，验证列映射与镜像
        let filled = temp_path("filled");
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.write_string(0, 0, "token").unwrap();
        sheet.write_string(0, 1, "account").unwrap();
        sheet.write_string(1, 0, "t1").unwrap();
        sheet.write_string(1, 1, "a1").unwrap();
        sheet.write_string(2, 0, "t2").unwrap();
        sheet.write_string(2, 1, "a2").unwrap();
        workbook.save(&filled).unwrap();

        let result = import_excel(&conn, 1, filled.to_str().unwrap()).unwrap();
        assert_eq!(result.inserted, 2);
        assert_eq!(result.skipped, 0);
        assert!(result.numeric_warnings.is_empty());
        let (value, account): (String, Option<String>) = conn
            .query_row(
                "SELECT value, account FROM data_pool_items ORDER BY id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(value, "t1");
        assert_eq!(account.as_deref(), Some("a1"));

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&filled);
    }
}
