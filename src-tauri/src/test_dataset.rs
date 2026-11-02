//! 测试数据集解析（CSV / JSON）
//!
//! 与前端批量运行里自实现的 CSV 语义保持一致：逗号分隔、双引号包裹、
//! `""` 转义双引号、允许 CRLF。JSON 数据集为对象数组，嵌套字段摊平成
//! `a.b.c` 便于 `{{a.b.c}}` 渲染，同时保留原始结构供 Python 侧读取。

use serde_json::{Map, Value};
use std::collections::HashMap;

/// 数据集的一行
#[derive(Debug, Clone, Default)]
pub struct DatasetRow {
    /// 行号，从 0 开始
    pub index: usize,
    /// 原始值（JSON 数据集保留嵌套结构）
    pub values: Map<String, Value>,
    /// 摊平后的字符串值，供模板渲染
    pub flat: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct DatasetTable {
    pub columns: Vec<String>,
    pub rows: Vec<DatasetRow>,
}

impl DatasetTable {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 解析数据集内容；`format` 只接受 csv / json
pub fn parse(format: &str, content: &str) -> Result<DatasetTable, String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "csv" => Ok(parse_csv(content)),
        "json" => parse_json(content),
        other => Err(format!("不支持的数据集格式：{other}")),
    }
}

/// 读取文件并解析，格式按扩展名推断
pub fn parse_file(path: &std::path::Path) -> Result<DatasetTable, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("读取数据集失败（{}）：{e}", path.display()))?;
    let format = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = if format == "json" { "json" } else { "csv" };
    parse(format, &content)
}

fn parse_json(content: &str) -> Result<DatasetTable, String> {
    let text = content.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err("数据集内容为空".to_string());
    }
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("数据集 JSON 解析失败：{e}"))?;
    let items = match value {
        Value::Array(items) => items,
        _ => return Err("数据集 JSON 必须是对象数组，例如 [{...}, {...}]".to_string()),
    };

    let mut columns: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let object = match item {
            Value::Object(map) => map,
            _ => return Err(format!("数据集第 {} 行不是 JSON 对象", index + 1)),
        };
        for key in object.keys() {
            if seen.insert(key.clone()) {
                columns.push(key.clone());
            }
        }
        let mut flat = HashMap::new();
        for (key, child) in &object {
            crate::util::flatten_json(key, child, &mut flat);
        }
        rows.push(DatasetRow {
            index,
            values: object,
            flat,
        });
    }

    Ok(DatasetTable { columns, rows })
}

/// 逐字符解析 CSV，支持引号内逗号 / 换行与 `""` 转义
pub fn parse_csv(content: &str) -> DatasetTable {
    let text = content.trim_start_matches('\u{feff}');
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => in_quotes = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {
                // CRLF 只按 \n 结束一行
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            other => field.push(other),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }

    // 去掉完全空白的行（例如文件末尾多余换行）
    records.retain(|row| !row.iter().all(|cell| cell.trim().is_empty()));
    if records.is_empty() {
        return DatasetTable::default();
    }

    let mut columns: Vec<String> = records.remove(0);
    for (index, column) in columns.iter_mut().enumerate() {
        *column = column.trim().to_string();
        if column.is_empty() {
            *column = format!("col_{}", index + 1);
        }
    }

    let rows = records
        .into_iter()
        .enumerate()
        .map(|(index, cells)| {
            let mut values = Map::new();
            let mut flat = HashMap::new();
            for (column_index, column) in columns.iter().enumerate() {
                let value = cells.get(column_index).cloned().unwrap_or_default();
                flat.insert(column.clone(), value.clone());
                values.insert(column.clone(), Value::String(value));
            }
            DatasetRow {
                index,
                values,
                flat,
            }
        })
        .collect();

    DatasetTable { columns, rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_handles_quotes_escapes_and_crlf() {
        let table = parse_csv("name,remark\r\nA,\"x,y\"\r\nB,\"say \"\"hi\"\"\"\r\n");
        assert_eq!(table.columns, vec!["name", "remark"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].flat["remark"], "x,y");
        assert_eq!(table.rows[1].flat["remark"], "say \"hi\"");
    }

    #[test]
    fn csv_tolerates_empty_header_and_short_rows() {
        let table = parse_csv("id,,note\n1,2\n");
        assert_eq!(table.columns[1], "col_2");
        assert_eq!(table.rows[0].flat["note"], "");
    }

    #[test]
    fn json_flattens_nested_objects() {
        let table = parse(
            "json",
            r#"[{"user":{"id":1,"name":"a"},"tags":["x"]},{"user":{"id":2,"name":"b"}}]"#,
        )
        .unwrap();
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].flat["user.id"], "1");
        assert_eq!(table.rows[0].flat["tags.0"], "x");
        assert_eq!(table.rows[1].flat["user.name"], "b");
        assert!(table.columns.contains(&"user".to_string()));
    }

    #[test]
    fn json_rejects_non_array_and_empty() {
        assert!(parse("json", "{}").is_err());
        assert!(parse("json", "").is_err());
        assert!(parse("yaml", "a: 1").is_err());
    }
}
