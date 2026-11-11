//! OpenAPI 3.0 子集的契约 diff
//!
//! 只覆盖常用子集，宁可漏报也不误报（技术方案 4.2）：
//! - 破坏性：删 path / operation / 状态码、新增必填参数、required false→true、
//!   参数类型或 format 变化、枚举值删除、删除响应字段、新增全局鉴权；
//! - 兼容性：新增 path / operation / 可选参数、required true→false、
//!   新增响应字段、required 放宽；
//! - 提示：描述 / summary / 示例、deprecated、枚举值新增、无实体导致 schema 缺失。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractChange {
    pub path: String,
    pub method: String,
    pub field: String,
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub before: Value,
    #[serde(default)]
    pub after: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractSummary {
    pub breaking: usize,
    pub compatible: usize,
    pub warnings: usize,
    pub paths: usize,
    pub operations: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractDiff {
    pub breaking: Vec<ContractChange>,
    pub compatible: Vec<ContractChange>,
    pub warnings: Vec<ContractChange>,
    /// 无返回实体的接口（"GET /path"），不计入破坏性判定
    pub schema_missing: Vec<String>,
    pub summary: ContractSummary,
}

impl ContractDiff {
    fn push(&mut self, level: Level, change: ContractChange) {
        match level {
            Level::Breaking => self.breaking.push(change),
            Level::Compatible => self.compatible.push(change),
            Level::Warning => self.warnings.push(change),
        }
    }

    fn finish(mut self, current: &Value) -> Self {
        let paths = current
            .get("paths")
            .and_then(Value::as_object)
            .map(|map| map.len())
            .unwrap_or(0);
        let operations = current
            .get("paths")
            .and_then(Value::as_object)
            .map(|map| {
                map.values()
                    .filter_map(Value::as_object)
                    .map(|object| {
                        object
                            .keys()
                            .filter(|key| METHODS.contains(&key.as_str()))
                            .count()
                    })
                    .sum()
            })
            .unwrap_or(0);
        self.summary = ContractSummary {
            breaking: self.breaking.len(),
            compatible: self.compatible.len(),
            warnings: self.warnings.len(),
            paths,
            operations,
        };
        self
    }
}

#[derive(Clone, Copy)]
enum Level {
    Breaking,
    Compatible,
    Warning,
}

struct Ctx<'a> {
    diff: &'a mut ContractDiff,
}

impl Ctx<'_> {
    #[allow(clippy::too_many_arguments)]
    fn note(
        &mut self,
        level: Level,
        path: &str,
        method: &str,
        field: &str,
        kind: &str,
        message: String,
        before: Value,
        after: Value,
    ) {
        self.diff.push(
            level,
            ContractChange {
                path: path.to_string(),
                method: method.to_string(),
                field: field.to_string(),
                kind: kind.to_string(),
                message,
                before,
                after,
            },
        );
    }
}

pub fn diff_specs(baseline: &Value, current: &Value) -> ContractDiff {
    let mut diff = ContractDiff::default();
    {
        let mut ctx = Ctx { diff: &mut diff };
        compare_global_security(&mut ctx, baseline, current);

        let empty = serde_json::Map::new();
        let base_paths = baseline
            .get("paths")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let cur_paths = current
            .get("paths")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let all_paths: BTreeSet<&String> = base_paths.keys().chain(cur_paths.keys()).collect();
        for path in all_paths {
            match (base_paths.get(path), cur_paths.get(path)) {
                (Some(_), None) => ctx.note(
                    Level::Breaking,
                    path,
                    "",
                    "",
                    "removed_path",
                    format!("接口路径已删除：{path}"),
                    Value::Null,
                    Value::Null,
                ),
                (None, Some(_)) => ctx.note(
                    Level::Compatible,
                    path,
                    "",
                    "",
                    "added_path",
                    format!("新增接口路径：{path}"),
                    Value::Null,
                    Value::Null,
                ),
                (Some(base), Some(cur)) => compare_path(&mut ctx, path, base, cur),
                (None, None) => {}
            }
        }
    }
    diff.finish(current)
}

fn compare_global_security(ctx: &mut Ctx, baseline: &Value, current: &Value) {
    let empty = |value: &Value| {
        value
            .get("security")
            .and_then(Value::as_array)
            .map(|items| items.is_empty())
            .unwrap_or(true)
    };
    if empty(baseline) && !empty(current) {
        ctx.note(
            Level::Breaking,
            "",
            "",
            "security",
            "global_security_added",
            "新增全局鉴权要求".to_string(),
            Value::Null,
            current.get("security").cloned().unwrap_or(Value::Null),
        );
    }
}

fn compare_path(ctx: &mut Ctx, path: &str, base: &Value, cur: &Value) {
    let (Some(base), Some(cur)) = (base.as_object(), cur.as_object()) else {
        return;
    };
    for method in METHODS {
        match (base.get(method), cur.get(method)) {
            (Some(_), None) => ctx.note(
                Level::Breaking,
                path,
                method,
                "",
                "removed_operation",
                format!("接口已删除：{} {}", method.to_uppercase(), path),
                Value::Null,
                Value::Null,
            ),
            (None, Some(_)) => ctx.note(
                Level::Compatible,
                path,
                method,
                "",
                "added_operation",
                format!("新增接口：{} {}", method.to_uppercase(), path),
                Value::Null,
                Value::Null,
            ),
            (Some(base_op), Some(cur_op)) => compare_operation(ctx, path, method, base_op, cur_op),
            (None, None) => {}
        }
    }
}

fn compare_operation(ctx: &mut Ctx, path: &str, method: &str, base: &Value, cur: &Value) {
    compare_text(ctx, path, method, base, cur, "summary");
    compare_text(ctx, path, method, base, cur, "description");
    if cur.get("deprecated") == Some(&Value::Bool(true))
        && base.get("deprecated") != Some(&Value::Bool(true))
    {
        ctx.note(
            Level::Warning,
            path,
            method,
            "deprecated",
            "deprecated",
            "接口被标记为 deprecated".to_string(),
            Value::Null,
            Value::Bool(true),
        );
    }
    compare_parameters(ctx, path, method, base, cur);
    compare_request_body(ctx, path, method, base, cur);
    compare_responses(ctx, path, method, base, cur);
}

fn compare_text(ctx: &mut Ctx, path: &str, method: &str, base: &Value, cur: &Value, key: &str) {
    let normal = |value: &Value| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let before = normal(base);
    let after = normal(cur);
    if before != after {
        ctx.note(
            Level::Warning,
            path,
            method,
            key,
            "description_changed",
            format!("{key} 发生变化"),
            Value::String(before),
            Value::String(after),
        );
    }
}

fn parameter_key(param: &Value) -> Option<String> {
    let name = param.get("name")?.as_str()?;
    let location = param.get("in").and_then(Value::as_str).unwrap_or("query");
    Some(format!("{location}:{name}"))
}

fn parameter_required(param: &Value) -> bool {
    param
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn parameters(value: &Value) -> BTreeMap<String, Value> {
    value
        .get("parameters")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parameter_key(item).map(|key| (key, item.clone())))
                .collect()
        })
        .unwrap_or_default()
}

fn compare_parameters(ctx: &mut Ctx, path: &str, method: &str, base: &Value, cur: &Value) {
    let base_params = parameters(base);
    let cur_params = parameters(cur);
    let all: BTreeSet<&String> = base_params.keys().chain(cur_params.keys()).collect();
    for key in all {
        let field = format!("parameters.{key}");
        match (base_params.get(key), cur_params.get(key)) {
            (Some(_), None) => ctx.note(
                Level::Warning,
                path,
                method,
                &field,
                "removed_parameter",
                format!("参数已删除：{key}"),
                Value::Null,
                Value::Null,
            ),
            (None, Some(param)) => {
                if parameter_required(param) {
                    ctx.note(
                        Level::Breaking,
                        path,
                        method,
                        &field,
                        "added_required_parameter",
                        format!("新增必填参数：{key}"),
                        Value::Null,
                        param.clone(),
                    );
                } else {
                    ctx.note(
                        Level::Compatible,
                        path,
                        method,
                        &field,
                        "added_parameter",
                        format!("新增可选参数：{key}"),
                        Value::Null,
                        param.clone(),
                    );
                }
            }
            (Some(base_param), Some(cur_param)) => {
                let was = parameter_required(base_param);
                let now = parameter_required(cur_param);
                if !was && now {
                    ctx.note(
                        Level::Breaking,
                        path,
                        method,
                        &field,
                        "parameter_became_required",
                        format!("参数由可选变为必填：{key}"),
                        Value::Bool(was),
                        Value::Bool(now),
                    );
                } else if was && !now {
                    ctx.note(
                        Level::Compatible,
                        path,
                        method,
                        &field,
                        "parameter_became_optional",
                        format!("参数由必填变为可选：{key}"),
                        Value::Bool(was),
                        Value::Bool(now),
                    );
                }
                let base_schema = base_param.get("schema").cloned().unwrap_or(Value::Null);
                let cur_schema = cur_param.get("schema").cloned().unwrap_or(Value::Null);
                compare_schema(
                    ctx,
                    path,
                    method,
                    &field,
                    &base_schema,
                    &cur_schema,
                    Side::Request,
                );
                compare_text(ctx, path, method, base_param, cur_param, "description");
            }
            (None, None) => {}
        }
    }
}

fn request_body_required(body: &Value) -> bool {
    body.get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn compare_request_body(ctx: &mut Ctx, path: &str, method: &str, base: &Value, cur: &Value) {
    let base_body = base.get("requestBody");
    let cur_body = cur.get("requestBody");
    match (base_body, cur_body) {
        (Some(base_body), None) => ctx.note(
            Level::Breaking,
            path,
            method,
            "requestBody",
            "request_body_removed",
            "请求体已删除".to_string(),
            base_body.clone(),
            Value::Null,
        ),
        (None, Some(cur_body)) => {
            if request_body_required(cur_body) {
                ctx.note(
                    Level::Breaking,
                    path,
                    method,
                    "requestBody",
                    "added_required_request_body",
                    "新增必填请求体".to_string(),
                    Value::Null,
                    cur_body.clone(),
                );
            } else {
                ctx.note(
                    Level::Compatible,
                    path,
                    method,
                    "requestBody",
                    "added_request_body",
                    "新增请求体".to_string(),
                    Value::Null,
                    cur_body.clone(),
                );
            }
        }
        (Some(base_body), Some(cur_body)) => {
            let was = request_body_required(base_body);
            let now = request_body_required(cur_body);
            if !was && now {
                ctx.note(
                    Level::Breaking,
                    path,
                    method,
                    "requestBody",
                    "request_body_became_required",
                    "请求体由可选变为必填".to_string(),
                    Value::Bool(was),
                    Value::Bool(now),
                );
            } else if was && !now {
                ctx.note(
                    Level::Compatible,
                    path,
                    method,
                    "requestBody",
                    "request_body_became_optional",
                    "请求体由必填变为可选".to_string(),
                    Value::Bool(was),
                    Value::Bool(now),
                );
            }
            compare_content(
                ctx,
                path,
                method,
                "requestBody",
                base_body,
                cur_body,
                Side::Request,
            );
        }
        (None, None) => {}
    }
}

fn compare_responses(ctx: &mut Ctx, path: &str, method: &str, base: &Value, cur: &Value) {
    let empty = serde_json::Map::new();
    let base_responses = base
        .get("responses")
        .and_then(Value::as_object)
        .unwrap_or(&empty)
        .clone();
    let cur_responses = cur
        .get("responses")
        .and_then(Value::as_object)
        .unwrap_or(&empty)
        .clone();
    let all: BTreeSet<&String> = base_responses.keys().chain(cur_responses.keys()).collect();
    for status in all {
        let field = format!("responses.{status}");
        match (base_responses.get(status), cur_responses.get(status)) {
            (Some(_), None) => ctx.note(
                Level::Breaking,
                path,
                method,
                &field,
                "removed_status",
                format!("响应状态码已删除：{status}"),
                Value::Null,
                Value::Null,
            ),
            (None, Some(response)) => ctx.note(
                Level::Compatible,
                path,
                method,
                &field,
                "added_status",
                format!("新增响应状态码：{status}"),
                Value::Null,
                response.clone(),
            ),
            (Some(base_response), Some(cur_response)) => {
                compare_text(
                    ctx,
                    path,
                    method,
                    base_response,
                    cur_response,
                    "description",
                );
                let base_has_schema = has_response_schema(base_response);
                let cur_has_schema = has_response_schema(cur_response);
                if status == "200" && !cur_has_schema {
                    ctx.diff
                        .schema_missing
                        .push(format!("{} {}", method.to_uppercase(), path));
                }
                match (base_has_schema, cur_has_schema) {
                    (true, false) => ctx.note(
                        Level::Warning,
                        path,
                        method,
                        &field,
                        "schema_missing",
                        "该接口无返回实体，响应侧不参与判定".to_string(),
                        Value::Null,
                        Value::Null,
                    ),
                    (false, true) => ctx.note(
                        Level::Compatible,
                        path,
                        method,
                        &field,
                        "schema_added",
                        "新增响应 schema".to_string(),
                        Value::Null,
                        Value::Null,
                    ),
                    (true, true) => compare_content(
                        ctx,
                        path,
                        method,
                        &field,
                        base_response,
                        cur_response,
                        Side::Response,
                    ),
                    (false, false) => {}
                }
            }
            (None, None) => {}
        }
    }
}

fn has_response_schema(response: &Value) -> bool {
    response
        .get("content")
        .and_then(Value::as_object)
        .map(|content| {
            content
                .values()
                .any(|media| media.get("schema").is_some_and(|schema| !schema.is_null()))
        })
        .unwrap_or(false)
}

fn compare_content(
    ctx: &mut Ctx,
    path: &str,
    method: &str,
    location: &str,
    base: &Value,
    cur: &Value,
    side: Side,
) {
    let empty = serde_json::Map::new();
    let base_content = base
        .get("content")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let cur_content = cur
        .get("content")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let all: BTreeSet<&String> = base_content.keys().chain(cur_content.keys()).collect();
    for mime in all {
        let field = format!("{location}.content.{mime}");
        match (base_content.get(mime), cur_content.get(mime)) {
            (Some(_), None) => ctx.note(
                if side == Side::Response {
                    Level::Breaking
                } else {
                    Level::Warning
                },
                path,
                method,
                &field,
                "removed_content_type",
                format!("内容类型已删除：{mime}"),
                Value::Null,
                Value::Null,
            ),
            (None, Some(_)) => ctx.note(
                Level::Compatible,
                path,
                method,
                &field,
                "added_content_type",
                format!("新增内容类型：{mime}"),
                Value::Null,
                Value::Null,
            ),
            (Some(base_media), Some(cur_media)) => {
                let base_schema = base_media.get("schema").cloned().unwrap_or(Value::Null);
                let cur_schema = cur_media.get("schema").cloned().unwrap_or(Value::Null);
                if !base_schema.is_null() && cur_schema.is_null() {
                    ctx.note(
                        Level::Warning,
                        path,
                        method,
                        &field,
                        "schema_missing",
                        "该接口无实体，schema 不参与判定".to_string(),
                        Value::Null,
                        Value::Null,
                    );
                } else if base_schema.is_null() && !cur_schema.is_null() {
                    ctx.note(
                        Level::Compatible,
                        path,
                        method,
                        &field,
                        "schema_added",
                        "新增 schema".to_string(),
                        Value::Null,
                        Value::Null,
                    );
                } else if !base_schema.is_null() && !cur_schema.is_null() {
                    compare_schema(ctx, path, method, &field, &base_schema, &cur_schema, side);
                }
            }
            (None, None) => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Request,
    Response,
}

#[allow(clippy::too_many_arguments)]
fn compare_schema(
    ctx: &mut Ctx,
    path: &str,
    method: &str,
    field: &str,
    base: &Value,
    cur: &Value,
    side: Side,
) {
    let (Some(base), Some(cur)) = (base.as_object(), cur.as_object()) else {
        return;
    };
    compare_type(ctx, path, method, field, base, cur);
    compare_nullable(ctx, path, method, field, base, cur);
    compare_enum(ctx, path, method, field, base, cur);

    let base_required = required_set(base);
    let cur_required = required_set(cur);
    let base_props = base
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let cur_props = cur
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let all: BTreeSet<&String> = base_props.keys().chain(cur_props.keys()).collect();
    for name in all {
        let child_field = format!("{field}.{name}");
        match (base_props.get(name), cur_props.get(name)) {
            (Some(_), None) => {
                if side == Side::Response {
                    ctx.note(
                        Level::Breaking,
                        path,
                        method,
                        &child_field,
                        "removed_response_field",
                        format!("响应字段已删除：{child_field}"),
                        Value::Null,
                        Value::Null,
                    );
                } else {
                    ctx.note(
                        Level::Warning,
                        path,
                        method,
                        &child_field,
                        "removed_request_field",
                        format!("请求字段已删除：{child_field}"),
                        Value::Null,
                        Value::Null,
                    );
                }
            }
            (None, Some(_)) => {
                if side == Side::Request && cur_required.contains(name) {
                    let value = cur_props.get(name).cloned().unwrap_or(Value::Null);
                    ctx.note(
                        Level::Breaking,
                        path,
                        method,
                        &child_field,
                        "added_required_field",
                        format!("新增必填字段：{child_field}"),
                        Value::Null,
                        value,
                    );
                } else {
                    let value = cur_props.get(name).cloned().unwrap_or(Value::Null);
                    ctx.note(
                        Level::Compatible,
                        path,
                        method,
                        &child_field,
                        "added_field",
                        format!("新增字段：{child_field}"),
                        Value::Null,
                        value,
                    );
                }
            }
            (Some(base_prop), Some(cur_prop)) => {
                if !base_required.contains(name) && cur_required.contains(name) {
                    ctx.note(
                        if side == Side::Request {
                            Level::Breaking
                        } else {
                            Level::Compatible
                        },
                        path,
                        method,
                        &child_field,
                        "field_became_required",
                        format!("字段变为必填：{child_field}"),
                        Value::Bool(false),
                        Value::Bool(true),
                    );
                } else if base_required.contains(name) && !cur_required.contains(name) {
                    ctx.note(
                        Level::Compatible,
                        path,
                        method,
                        &child_field,
                        "field_became_optional",
                        format!("字段变为可选：{child_field}"),
                        Value::Bool(true),
                        Value::Bool(false),
                    );
                }
                compare_schema(ctx, path, method, &child_field, base_prop, cur_prop, side);
            }
            (None, None) => {}
        }
    }

    match (base.get("items"), cur.get("items")) {
        (Some(base_items), Some(cur_items)) => {
            let items_field = format!("{field}[]");
            compare_schema(ctx, path, method, &items_field, base_items, cur_items, side);
        }
        (Some(_), None) => ctx.note(
            Level::Warning,
            path,
            method,
            &format!("{field}[]"),
            "items_removed",
            "数组元素定义已移除".to_string(),
            Value::Null,
            Value::Null,
        ),
        (None, Some(_)) => ctx.note(
            Level::Compatible,
            path,
            method,
            &format!("{field}[]"),
            "items_added",
            "补充数组元素定义".to_string(),
            Value::Null,
            Value::Null,
        ),
        (None, None) => {}
    }
}

fn required_set(schema: &serde_json::Map<String, Value>) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn compare_type(
    ctx: &mut Ctx,
    path: &str,
    method: &str,
    field: &str,
    base: &serde_json::Map<String, Value>,
    cur: &serde_json::Map<String, Value>,
) {
    let base_type = base.get("type").and_then(Value::as_str);
    let cur_type = cur.get("type").and_then(Value::as_str);
    match (base_type, cur_type) {
        (Some(before), Some(after)) if !compatible_type(before, after) => ctx.note(
            Level::Breaking,
            path,
            method,
            field,
            "type_changed",
            format!("类型由 {before} 变为 {after}"),
            Value::String(before.to_string()),
            Value::String(after.to_string()),
        ),
        (Some(_), None) => ctx.note(
            Level::Warning,
            path,
            method,
            field,
            "schema_relaxed",
            "类型定义被移除（视为 unknown）".to_string(),
            Value::Null,
            Value::Null,
        ),
        (None, Some(after)) => ctx.note(
            Level::Compatible,
            path,
            method,
            field,
            "schema_specified",
            format!("补充类型定义：{after}"),
            Value::Null,
            Value::String(after.to_string()),
        ),
        _ => {}
    }
    let base_format = base.get("format").and_then(Value::as_str);
    let cur_format = cur.get("format").and_then(Value::as_str);
    match (base_format, cur_format) {
        (Some(before), Some(after)) if before != after => ctx.note(
            Level::Breaking,
            path,
            method,
            field,
            "format_changed",
            format!("format 由 {before} 变为 {after}"),
            Value::String(before.to_string()),
            Value::String(after.to_string()),
        ),
        (Some(before), None) => ctx.note(
            Level::Compatible,
            path,
            method,
            field,
            "format_removed",
            format!("移除 format：{before}"),
            Value::String(before.to_string()),
            Value::Null,
        ),
        (None, Some(after)) => ctx.note(
            Level::Compatible,
            path,
            method,
            field,
            "format_added",
            format!("新增 format：{after}"),
            Value::Null,
            Value::String(after.to_string()),
        ),
        _ => {}
    }
}

fn compatible_type(before: &str, after: &str) -> bool {
    before == after
        || (matches!(before, "integer" | "number") && matches!(after, "integer" | "number"))
}

fn compare_nullable(
    ctx: &mut Ctx,
    path: &str,
    method: &str,
    field: &str,
    base: &serde_json::Map<String, Value>,
    cur: &serde_json::Map<String, Value>,
) {
    let before = base
        .get("nullable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let after = cur
        .get("nullable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !before && after {
        ctx.note(
            Level::Compatible,
            path,
            method,
            field,
            "nullable_added",
            "字段允许为 null".to_string(),
            Value::Bool(before),
            Value::Bool(after),
        );
    } else if before && !after {
        ctx.note(
            Level::Breaking,
            path,
            method,
            field,
            "nullable_removed",
            "字段不再允许为 null".to_string(),
            Value::Bool(before),
            Value::Bool(after),
        );
    }
}

fn enum_values(schema: &serde_json::Map<String, Value>) -> BTreeSet<String> {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(enum_key).collect())
        .unwrap_or_default()
}

fn enum_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn compare_enum(
    ctx: &mut Ctx,
    path: &str,
    method: &str,
    field: &str,
    base: &serde_json::Map<String, Value>,
    cur: &serde_json::Map<String, Value>,
) {
    let before = enum_values(base);
    let after = enum_values(cur);
    if before.is_empty() {
        return;
    }
    if after.is_empty() {
        ctx.note(
            Level::Warning,
            path,
            method,
            field,
            "enum_relaxed",
            "枚举约束被移除".to_string(),
            Value::Null,
            Value::Null,
        );
        return;
    }
    for removed in before.difference(&after) {
        ctx.note(
            Level::Breaking,
            path,
            method,
            field,
            "enum_value_removed",
            format!("枚举值已删除：{removed}"),
            Value::String(removed.clone()),
            Value::Null,
        );
    }
    for added in after.difference(&before) {
        ctx.note(
            Level::Warning,
            path,
            method,
            field,
            "enum_value_added",
            format!("新增枚举值：{added}"),
            Value::Null,
            Value::String(added.clone()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(paths: Value) -> Value {
        json!({ "openapi": "3.0.3", "paths": paths })
    }

    fn kinds(items: &[ContractChange]) -> Vec<&str> {
        items.iter().map(|item| item.kind.as_str()).collect()
    }

    fn response_schema(schema: Value) -> Value {
        spec(json!({
            "/a": {
                "get": {
                    "responses": {
                        "200": {
                            "description": "OK",
                            "content": { "application/json": { "schema": schema } }
                        }
                    }
                }
            }
        }))
    }

    #[test]
    fn identical_specs_have_empty_diff() {
        let value = spec(json!({
            "/a": { "get": { "responses": { "200": { "description": "OK" } } } }
        }));
        let diff = diff_specs(&value, &value);
        assert!(diff.breaking.is_empty());
        assert!(diff.compatible.is_empty());
        assert!(diff.warnings.is_empty());
        assert_eq!(diff.summary.breaking, 0);
        assert_eq!(diff.summary.paths, 1);
        assert_eq!(diff.summary.operations, 1);
    }

    #[test]
    fn path_and_operation_removal_is_breaking() {
        let base = spec(json!({
            "/a": { "get": {}, "post": {} },
            "/b": { "get": {} }
        }));
        let cur = spec(json!({ "/a": { "get": {} } }));
        let diff = diff_specs(&base, &cur);
        assert!(kinds(&diff.breaking).contains(&"removed_path"));
        assert!(kinds(&diff.breaking).contains(&"removed_operation"));
        assert_eq!(diff.summary.breaking, 2);
    }

    #[test]
    fn added_path_is_compatible() {
        let diff = diff_specs(&spec(json!({})), &spec(json!({ "/a": { "get": {} } })));
        assert_eq!(kinds(&diff.compatible), vec!["added_path"]);
    }

    #[test]
    fn required_parameter_rules() {
        let base = spec(json!({
            "/a": { "get": { "parameters": [
                { "name": "page", "in": "query", "required": false, "schema": { "type": "string" } }
            ] } }
        }));
        let cur = spec(json!({
            "/a": { "get": { "parameters": [
                { "name": "page", "in": "query", "required": true, "schema": { "type": "string" } },
                { "name": "token", "in": "header", "required": true, "schema": { "type": "string" } },
                { "name": "size", "in": "query", "required": false, "schema": { "type": "string" } }
            ] } }
        }));
        let diff = diff_specs(&base, &cur);
        let breaking = kinds(&diff.breaking);
        assert!(breaking.contains(&"parameter_became_required"));
        assert!(breaking.contains(&"added_required_parameter"));
        assert_eq!(kinds(&diff.compatible), vec!["added_parameter"]);
        assert_eq!(diff.summary.breaking, 2);
    }

    #[test]
    fn type_and_format_changes_are_breaking_but_number_integer_is_not() {
        let base = response_schema(json!({ "type": "number" }));
        let cur = response_schema(json!({ "type": "integer" }));
        assert!(diff_specs(&base, &cur).breaking.is_empty());

        let base = response_schema(json!({ "type": "string", "format": "date" }));
        let cur = response_schema(json!({ "type": "string", "format": "date-time" }));
        let diff = diff_specs(&base, &cur);
        assert_eq!(kinds(&diff.breaking), vec!["format_changed"]);

        let base = response_schema(json!({ "type": "string" }));
        let cur = response_schema(json!({ "type": "number" }));
        assert_eq!(
            kinds(&diff_specs(&base, &cur).breaking),
            vec!["type_changed"]
        );
    }

    #[test]
    fn enum_value_changes_are_split_between_breaking_and_warning() {
        let base = response_schema(json!({ "type": "string", "enum": ["a", "b"] }));
        let cur = response_schema(json!({ "type": "string", "enum": ["b", "c"] }));
        let diff = diff_specs(&base, &cur);
        assert_eq!(kinds(&diff.breaking), vec!["enum_value_removed"]);
        assert_eq!(kinds(&diff.warnings), vec!["enum_value_added"]);
    }

    #[test]
    fn response_field_removal_is_breaking_and_addition_is_compatible() {
        let base = response_schema(json!({
            "type": "object",
            "properties": { "id": { "type": "number" }, "name": { "type": "string" } },
            "required": ["id"]
        }));
        let cur = response_schema(json!({
            "type": "object",
            "properties": { "id": { "type": "number" }, "email": { "type": "string" } },
            "required": ["id"]
        }));
        let diff = diff_specs(&base, &cur);
        assert_eq!(kinds(&diff.breaking), vec!["removed_response_field"]);
        assert_eq!(kinds(&diff.compatible), vec!["added_field"]);
    }

    #[test]
    fn required_request_field_is_breaking_and_optional_is_not() {
        let base = spec(json!({
            "/a": { "post": { "requestBody": { "content": { "application/json": {
                "schema": { "type": "object", "properties": { "id": { "type": "number" } } }
            } } } } }
        }));
        let cur = spec(json!({
            "/a": { "post": { "requestBody": { "content": { "application/json": {
                "schema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "number" },
                        "name": { "type": "string" },
                        "note": { "type": "string" }
                    },
                    "required": ["name"]
                }
            } } } } }
        }));
        let diff = diff_specs(&base, &cur);
        assert_eq!(kinds(&diff.breaking), vec!["added_required_field"]);
        assert_eq!(diff.compatible.len(), 1);
    }

    #[test]
    fn removed_status_is_breaking() {
        let base = spec(json!({
            "/a": { "get": { "responses": {
                "200": { "description": "OK" }, "404": { "description": "no" }
            } } }
        }));
        let cur = spec(json!({
            "/a": { "get": { "responses": { "200": { "description": "OK" } } } }
        }));
        assert_eq!(
            kinds(&diff_specs(&base, &cur).breaking),
            vec!["removed_status"]
        );
    }

    #[test]
    fn missing_entity_is_schema_missing_not_breaking() {
        let base = response_schema(
            json!({ "type": "object", "properties": { "id": { "type": "number" } } }),
        );
        let cur = spec(json!({
            "/a": { "get": { "responses": { "200": { "description": "OK" } } } }
        }));
        let diff = diff_specs(&base, &cur);
        assert!(diff.breaking.is_empty());
        assert_eq!(kinds(&diff.warnings), vec!["schema_missing"]);
        assert_eq!(diff.schema_missing, vec!["GET /a".to_string()]);
    }

    #[test]
    fn description_and_security_changes() {
        let base = spec(json!({
            "/a": { "get": { "summary": "A", "responses": { "200": { "description": "OK" } } } }
        }));
        let cur = spec(json!({
            "/a": { "get": { "summary": "B", "responses": { "200": { "description": "OK" } } } }
        }));
        let diff = diff_specs(&base, &cur);
        assert_eq!(kinds(&diff.warnings), vec!["description_changed"]);

        let mut base = spec(json!({}));
        let mut cur = spec(json!({}));
        cur["security"] = json!([{ "bearer": [] }]);
        base["security"] = json!([]);
        assert_eq!(
            kinds(&diff_specs(&base, &cur).breaking),
            vec!["global_security_added"]
        );
    }
}
