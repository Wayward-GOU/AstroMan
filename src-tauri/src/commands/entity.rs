//! 接口实体的读取与手动生成命令（技术方案 5.3.1 / 5.8.5）

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::State;

use crate::entity::model::EntityDoc;
use crate::entity::{infer, merge};
use crate::state::AppState;
use crate::util::{err, now_ms};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEntities {
    pub request_entity: Option<EntityDoc>,
    pub response_entity: Option<EntityDoc>,
    pub warnings: Vec<String>,
}

fn decode_column(
    raw: Option<String>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<EntityDoc> {
    let raw = raw?;
    if raw.trim().is_empty() {
        return None;
    }
    match EntityDoc::decode(&raw) {
        Ok(doc) => Some(doc),
        Err(error) => {
            warnings.push(format!("{label}数据已忽略：{error}"));
            None
        }
    }
}

fn load_entities(conn: &Connection, request_id: i64) -> Result<RequestEntities, String> {
    let row = conn
        .query_row(
            "SELECT request_entity, response_entity FROM requests WHERE id = ?1",
            params![request_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(err)?;
    let Some((request_entity, response_entity)) = row else {
        return Err("请求不存在".to_string());
    };
    let mut warnings = Vec::new();
    Ok(RequestEntities {
        request_entity: decode_column(request_entity, "请求实体", &mut warnings),
        response_entity: decode_column(response_entity, "返回实体", &mut warnings),
        warnings,
    })
}

#[tauri::command]
pub fn request_entities_get(
    state: State<'_, AppState>,
    request_id: i64,
) -> Result<RequestEntities, String> {
    let conn = state.pool.get().map_err(err)?;
    load_entities(&conn, request_id)
}

#[tauri::command]
pub fn request_entity_generate(
    state: State<'_, AppState>,
    request_id: i64,
    target: String,
    body: Option<String>,
    body_type: Option<String>,
) -> Result<RequestEntities, String> {
    let conn = state.pool.get().map_err(err)?;
    let base = conn
        .query_row(
            "SELECT body, body_type FROM requests WHERE id = ?1",
            params![request_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(1)?
                        .unwrap_or_else(|| "none".to_string()),
                ))
            },
        )
        .optional()
        .map_err(err)?;
    let Some((_saved_body, saved_type)) = base else {
        return Err("请求不存在".to_string());
    };

    let target = target.trim().to_ascii_lowercase();
    let inferred = match target.as_str() {
        "request" => {
            let explicit = body
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
            let (text, kind) = if let Some(text) = explicit {
                let kind = body_type
                    .as_deref()
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .unwrap_or(&saved_type)
                    .to_string();
                (text, normalize_body_type(&kind))
            } else {
                let run = conn
                    .query_row(
                        "SELECT body, body_type FROM request_runs
                          WHERE request_id = ?1 AND body IS NOT NULL AND TRIM(body) <> ''
                          ORDER BY id DESC LIMIT 1",
                        params![request_id],
                        |row| {
                            Ok((
                                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                                row.get::<_, Option<String>>(1)?
                                    .unwrap_or_else(|| "json".to_string()),
                            ))
                        },
                    )
                    .optional()
                    .map_err(err)?;
                match run {
                    Some((text, kind)) => (text, normalize_body_type(&kind)),
                    None => {
                        return Err(
                            "E_ENTITY_PARSE: 当前没有可推断的请求体，请先填写 Body 或发送一次"
                                .to_string(),
                        )
                    }
                }
            };
            if text.trim().is_empty() {
                return Err("E_ENTITY_PARSE: 请求体为空，无法生成请求实体".to_string());
            }
            infer::infer_body(&kind, &text)?
        }
        "response" => {
            let text = conn
                .query_row(
                    "SELECT response_body FROM request_runs
                      WHERE request_id = ?1
                        AND response_status BETWEEN 200 AND 299
                        AND response_body IS NOT NULL
                        AND TRIM(response_body) <> ''
                        AND response_body_truncated = 0
                      ORDER BY id DESC LIMIT 1",
                    params![request_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .map_err(err)?
                .flatten()
                .unwrap_or_default();
            if text.trim().is_empty() {
                return Err("E_ENTITY_PARSE: 没有可用于生成返回实体的成功运行记录".to_string());
            }
            infer::infer_json(&text)?
        }
        _ => {
            return Err(format!(
                "E_ENTITY_TARGET: 不支持的实体目标「{target}」，只能是 request 或 response"
            ))
        }
    };

    let current = load_entities(&conn, request_id)?;
    let existing = if target == "request" {
        current.request_entity
    } else {
        current.response_entity
    };
    let merged = merge::merge(existing, inferred.root, true, now_ms());
    let encoded = merged.doc.encode()?;
    let column = if target == "request" {
        "request_entity"
    } else {
        "response_entity"
    };
    conn.execute(
        &format!("UPDATE requests SET {column} = ?1 WHERE id = ?2"),
        params![encoded, request_id],
    )
    .map_err(err)?;

    let mut result = load_entities(&conn, request_id)?;
    let mut warnings = inferred.warnings;
    warnings.extend(merged.warnings);
    warnings.extend(result.warnings);
    result.warnings = warnings;
    Ok(result)
}

fn normalize_body_type(kind: &str) -> String {
    match kind {
        "" | "none" | "binary" => "json".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_type_falls_back_to_json_for_plain_requests() {
        assert_eq!(normalize_body_type("none"), "json");
        assert_eq!(normalize_body_type("binary"), "json");
        assert_eq!(normalize_body_type(""), "json");
        assert_eq!(normalize_body_type("form"), "form");
    }

    #[test]
    fn broken_entity_column_is_reported_not_fatal() {
        let mut warnings = Vec::new();
        assert!(decode_column(Some("{".into()), "请求实体", &mut warnings).is_none());
        assert_eq!(warnings.len(), 1);
        assert!(crate::entity::decode_lenient(Some("{")).is_none());
    }
}
