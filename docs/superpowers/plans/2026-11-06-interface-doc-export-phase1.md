# 接口文档导出（阶段一 A+B）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 AstroMan 中实现「准备 → 预览 → 导出」的接口文档闭环，支持单接口 / 集合 / 项目三级、Markdown 与 HTML 两种格式、全屏预览抽屉与稀疏覆盖层编辑。

**Architecture:** Rust 侧新增 `docs` 模块：`builder` 一次性读取 SQLite、解析环境变量、按值脱敏、构建 IR（`DocBundle`），`DocsCache` 按 `draft_id` 缓存 IR；前端只拿瘦身大纲，编辑产生 `DocEditOverlay`，渲染与导出由后端合并覆盖层后交给纯函数渲染器。前端新增全屏抽屉与编辑树，入口挂在项目页、集合节点与请求编辑页。

**Tech Stack:** Tauri v2（Rust + `rusqlite`）、Vue 3 `<script setup>`、ant-design-vue 4、Vitest、markdown-it。

**Spec:** `docx/AstroMan 接口文档导出技术方案.md`（本次只实现阶段一 A+B；PDF / OpenAPI 三级 / DB v9 留给下一份计划）

## Global Constraints

- 脱敏发生在字段产出阶段，统一掩码 `••••••`；不得在渲染后的文本上做替换。
- 产物中不得出现任何真实凭据（auth token / basic 口令 / clientSecret / Cookie / `X-API-Key` 值 / URL userinfo / `is_secret=1` 的环境变量值）。
- 渲染器是纯函数：`&DocBundle → String`，不读库、不写文件。
- 三个命令全部使用 `#[tauri::command(async)]`；`docs_render` 不读数据库。
- 单次导出请求数上限 5000；单个 Body 输出上限 64 KB；大纲序列化上限 5 MB。
- `DocsOptions` 必须手写 `impl Default`；禁止 `derive(Default)`（bool 默认值会变成 false）。
- 覆盖层只保存改过的字段；不保存 `draft_id`，不保存 IR；`localStorage` key 为 `astroman:docs-overlay:{scope}:{targetId}:{environmentId}`。
- 不引入新 Rust 依赖、不引入新的前端依赖。
- 本次范围不包含 PDF、OpenAPI 渲染与 DB 迁移；`docs_export` 只接受 `markdown / html`。
- 按用户偏好：不执行 `git commit` / `git push`，完成后由用户统一提交。

---

## 文件结构

新增：

- `src-tauri/src/docs/mod.rs` — 子模块注册
- `src-tauri/src/docs/model.rs` — IR、大纲、覆盖层、命令载荷与结果
- `src-tauri/src/docs/masking.rs` — 敏感名判定、值级掩码、JSON 掩码、Body 截断
- `src-tauri/src/docs/examples.rs` — 确定性示例生成
- `src-tauri/src/docs/builder.rs` — 读库、环境解析、示例链、IR 组装、大纲
- `src-tauri/src/docs/cache.rs` — `DocsCache`（TTL / LRU）
- `src-tauri/src/docs/overlay.rs` — 覆盖层合并与顺序 / 排除
- `src-tauri/src/docs/render_markdown.rs`
- `src-tauri/src/docs/render_html.rs`
- `src-tauri/src/commands/docs.rs` — 三个命令与文件写入
- `src/types/docs.ts`
- `src/utils/docsExport.ts`
- `src/utils/__tests__/docsExport.spec.ts`
- `src/components/DocsOutlineNode.vue`
- `src/components/DocsPreviewDrawer.vue`

修改：

- `src-tauri/src/lib.rs` — `pub mod docs;`、`invoke_handler`
- `src-tauri/src/state.rs` — `AppState.docs_cache`
- `src-tauri/src/commands/mod.rs` — `pub mod docs;`
- `src-tauri/src/commands/request.rs` — `map_detail` 改为 `pub(crate)`
- `src/api/index.ts` — 三个命令
- `src/views/Project.vue` — 项目/集合入口
- `src/views/RequestEditor.vue` — 单接口入口

---

### Task 1: Rust IR 模型、脱敏与示例生成

**Files:**
- Create: `src-tauri/src/docs/mod.rs`
- Create: `src-tauri/src/docs/model.rs`
- Create: `src-tauri/src/docs/masking.rs`
- Create: `src-tauri/src/docs/examples.rs`
- Modify: `src-tauri/src/lib.rs:1-20`（在 `pub mod db;` 附近注册）

**Interfaces:**
- Produces:
  - `docs::model::{DocBundle, DocGroup, DocRequest, DocParam, DocBody, DocAuth, DocScopeInfo, DocEnvironment, DocEnvVar, DocsPreparePayload, DocsOptions, DocOutline, DocOutlineGroup, DocOutlineRequest, DocOutlineField, DocEditOverlay, DocEditMeta, DocNodeEdit, DocsRenderPayload, DocsRenderResult, DocsExportSource, DocsExportPayload, DocsExportResult, DocsPrepareResult, DocMetaDefaults, MASK}`
  - `masking::{is_sensitive_name, mask_userinfo, truncate_body}`
  - `masking::SecretSet::{add, extend, mask_text, mask_json, len}`
  - `examples::auto_value(name: &str, value_hint: &str, seq: usize, request_id: i64) -> String`

- [ ] **Step 1: 写失败测试（`masking.rs` / `examples.rs` 底部）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_names_use_whole_words() {
        for name in ["api_key", "apiKey", "accessToken", "X-API-Key", "password", "clientSecret"] {
            assert!(is_sensitive_name(name), "{name} 应命中");
        }
        for name in ["keyword", "tokenizer", "monkey", "pageSize"] {
            assert!(!is_sensitive_name(name), "{name} 不应命中");
        }
    }

    #[test]
    fn secret_set_masks_long_values_first_and_short_values_only_exactly() {
        let mut set = SecretSet::default();
        set.add("abc");
        set.add("abcdef");
        set.add("");
        assert_eq!(set.mask_text("x abcdef y"), "x •••••• y");
        assert_eq!(set.mask_text("abc"), "••••••");
        assert_eq!(set.mask_text("abcd"), "abcd");
    }

    #[test]
    fn json_masking_replaces_values_without_breaking_structure() {
        let mut set = SecretSet::default();
        set.add("s3cr3t-value");
        let out = set.mask_json(r#"{"password":"p@ss","note":"prefix-s3cr3t-value-suffix","n":1}"#);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["password"], "••••••");
        assert_eq!(value["note"], "prefix-••••••-suffix");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn userinfo_is_masked() {
        assert_eq!(mask_userinfo("https://u:p@example.com/a"), "https://••••••@example.com/a");
        assert_eq!(mask_userinfo("https://example.com/a"), "https://example.com/a");
    }

    #[test]
    fn auto_values_are_deterministic() {
        assert_eq!(auto_value("page", "", 0, 7), "1");
        assert_eq!(auto_value("size", "", 1, 7), "10");
        assert_eq!(auto_value("email", "", 2, 7), "user10001@example.com");
        assert_eq!(auto_value("userId", "", 0, 7), "10001");
        assert_eq!(auto_value("userId", "", 0, 7), auto_value("userId", "", 0, 7));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::masking -- --nocapture`
Expected: 编译失败，`no module named docs` / `is_sensitive_name not found`

- [ ] **Step 3: 写 `docs/mod.rs`**

```rust
pub mod builder;
pub mod cache;
pub mod examples;
pub mod masking;
pub mod model;
pub mod overlay;
pub mod render_html;
pub mod render_markdown;
```

同时把 `src-tauri/src/lib.rs` 里的 `pub mod db;` 改成：

```rust
pub mod db;
pub mod docs;
```

- [ ] **Step 4: 写 `docs/model.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MASK: &str = "••••••";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocBundle {
    pub generated_at: i64,
    pub app_version: String,
    pub revision: String,
    pub subtitle: String,
    pub scope: DocScopeInfo,
    pub groups: Vec<DocGroup>,
    pub environments: Vec<DocEnvironment>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocScopeInfo {
    pub kind: String,
    pub title: String,
    pub project_name: String,
    pub target_id: i64,
    pub environment_id: Option<i64>,
    pub environment_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocGroup {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub path: Vec<String>,
    pub requests: Vec<DocRequest>,
    pub children: Vec<DocGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRequest {
    pub id: i64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub description: String,
    pub query: Vec<DocParam>,
    pub path_params: Vec<DocParam>,
    pub headers: Vec<DocParam>,
    pub auth: DocAuth,
    pub body: DocBody,
    pub scripts: Option<DocScripts>,
    pub assertions: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocParam {
    pub name: String,
    pub value: String,
    pub required: bool,
    pub location: String,
    pub example_source: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocAuth {
    pub kind: String,
    pub add_to: String,
    pub key_name: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocBody {
    /// none | json | raw | text | form | multipart | binary | graphql
    pub kind: String,
    pub text: String,
    pub fields: Vec<DocParam>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocScripts {
    pub pre: String,
    pub post: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEnvironment {
    pub name: String,
    pub is_active: bool,
    pub variables: Vec<DocEnvVar>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEnvVar {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsPreparePayload {
    pub scope: String,
    pub target_id: i64,
    pub environment_id: Option<i64>,
    #[serde(default)]
    pub options: DocsOptions,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct DocsOptions {
    pub include_environment: bool,
    pub include_scripts: bool,
    pub include_assertions: bool,
    pub include_response_example: bool,
    pub resolve_environment: bool,
    pub use_run_examples: bool,
    pub auto_examples: bool,
    pub run_examples_max_age_days: u32,
    pub page_break_per_request: bool,
    pub include_generated_at: bool,
}

impl Default for DocsOptions {
    fn default() -> Self {
        Self {
            include_environment: true,
            include_scripts: false,
            include_assertions: false,
            include_response_example: false,
            resolve_environment: true,
            use_run_examples: true,
            auto_examples: true,
            run_examples_max_age_days: 30,
            page_break_per_request: true,
            include_generated_at: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutline {
    pub groups: Vec<DocOutlineGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineGroup {
    pub key: String,
    pub name: String,
    pub path: Vec<String>,
    pub requests: Vec<DocOutlineRequest>,
    pub children: Vec<DocOutlineGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineRequest {
    pub key: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub description: String,
    pub included: bool,
    pub params: Vec<DocOutlineField>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocOutlineField {
    pub key: String,
    pub name: String,
    pub location: String,
    pub value: String,
    pub source: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEditOverlay {
    pub revision: String,
    #[serde(default)]
    pub meta: DocEditMeta,
    #[serde(default)]
    pub nodes: BTreeMap<String, DocNodeEdit>,
    #[serde(default)]
    pub excluded: BTreeSet<String>,
    #[serde(default)]
    pub order: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocNodeEdit {
    pub description: Option<String>,
    #[serde(default)]
    pub example_values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEditMeta {
    pub title: Option<String>,
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocMetaDefaults {
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsPrepareResult {
    pub draft_id: String,
    pub revision: String,
    pub meta: DocMetaDefaults,
    pub outline: DocOutline,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsRenderPayload {
    pub draft_id: String,
    pub overlay: DocEditOverlay,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsRenderResult {
    pub content: String,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DocsExportSource {
    Scope(DocsPreparePayload),
    Draft {
        draft_id: String,
        overlay: DocEditOverlay,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportPayload {
    pub format: String,
    pub path: String,
    pub source: DocsExportSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportResult {
    pub path: String,
    pub format: String,
    pub scope: String,
    pub request_count: usize,
    pub collection_count: usize,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub pdf_engine: Option<String>,
    pub fallback_html_path: Option<String>,
}
```

- [ ] **Step 5: 写 `docs/masking.rs`**

```rust
use super::model::MASK;
use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct SecretSet {
    values: Vec<String>,
}

impl SecretSet {
    pub fn add(&mut self, value: &str) {
        let value = value.trim();
        if value.is_empty() || value == MASK {
            return;
        }
        if !self.values.iter().any(|item| item == value) {
            self.values.push(value.to_string());
        }
    }

    pub fn extend(&mut self, values: impl IntoIterator<Item = String>) {
        for value in values {
            self.add(&value);
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    fn sorted(&self) -> Vec<&str> {
        let mut list: Vec<&str> = self.values.iter().map(String::as_str).collect();
        list.sort_by_key(|item| std::cmp::Reverse(item.chars().count()));
        list
    }

    pub fn mask_text(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let mut out = text.to_string();
        for secret in self.sorted() {
            if secret.chars().count() >= 4 {
                out = out.replace(secret, MASK);
            } else if out == secret {
                out = MASK.to_string();
            }
            if out == MASK {
                break;
            }
        }
        out
    }

    pub fn mask_json(&self, raw: &str) -> String {
        if raw.trim().is_empty() {
            return raw.to_string();
        }
        let Ok(mut value) = serde_json::from_str::<Value>(raw) else {
            return self.mask_text(raw);
        };
        self.mask_json_value(&mut value);
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| self.mask_text(raw))
    }

    fn mask_json_value(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.mask_text(text),
            Value::Array(items) => {
                for item in items {
                    self.mask_json_value(item);
                }
            }
            Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if is_sensitive_name(key) {
                        *item = Value::String(MASK.to_string());
                    } else {
                        self.mask_json_value(item);
                    }
                }
            }
            _ => {}
        }
    }
}

/// 敏感名称判定：按 `_` / `-` / `.` / 空格与 camelCase 分词后整词比较
pub fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    ) {
        return true;
    }
    split_words(name).iter().any(|word| {
        matches!(
            word.as_str(),
            "token" | "password" | "passwd" | "secret" | "key" | "credential"
        )
    })
}

fn split_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if matches!(ch, '_' | '-' | '.' | ' ') {
            if !current.is_empty() {
                words.push(current.to_ascii_lowercase());
                current.clear();
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() {
            if prev_lower && !current.is_empty() {
                words.push(current.to_ascii_lowercase());
                current.clear();
            }
            current.push(ch);
            prev_lower = false;
        } else {
            current.push(ch);
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    if !current.is_empty() {
        words.push(current.to_ascii_lowercase());
    }
    words
}

/// URL userinfo 整体掩码：`https://user:pass@host` → `https://••••••@host`
pub fn mask_userinfo(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        if let Some(at) = rest.find('@') {
            let authority = &rest[..at];
            if authority.contains(':') {
                return format!("{}://{}@{}", &url[..scheme_end], MASK, &rest[at + 1..]);
            }
        }
    }
    url.to_string()
}

pub fn truncate_body(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n（已截断）", &text[..end])
}
```

- [ ] **Step 6: 写 `docs/examples.rs`**

```rust
fn seed_value(request_id: i64, seq: usize) -> u64 {
    (request_id.max(1) as u64)
        .wrapping_mul(131)
        .wrapping_add(seq as u64 * 17)
}

fn pseudo_uuid(seed: u64) -> String {
    let a = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = a.rotate_left(17).wrapping_add(0xA5A5_A5A5_A5A5_A5A5);
    let hex = format!("{a:016x}{b:016x}");
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

/// 确定性示例值：同一接口、同一字段序号，反复导出结果一致
pub fn auto_value(name: &str, value_hint: &str, seq: usize, request_id: i64) -> String {
    let lower = name.to_ascii_lowercase();
    let hint = value_hint.to_ascii_lowercase();
    let seed = seed_value(request_id, seq);

    if lower.contains("email") {
        return "user10001@example.com".to_string();
    }
    if lower.contains("phone") || lower.contains("mobile") {
        return "13800138000".to_string();
    }
    if lower.contains("price") || lower.contains("amount") || lower.contains("total") {
        return "99.90".to_string();
    }
    if lower.contains("uuid") || lower.contains("guid") {
        return pseudo_uuid(seed);
    }
    if lower == "page" || lower == "offset" || lower.ends_with("_page") {
        return "1".to_string();
    }
    if matches!(lower.as_str(), "size" | "limit" | "pagesize" | "page_size" | "per_page") {
        return "10".to_string();
    }
    if lower.contains("datetime") || lower.contains("time") {
        return "2026-01-01T12:00:00+08:00".to_string();
    }
    if lower.contains("date") {
        return "2026-01-01".to_string();
    }
    if lower.contains("name") || lower.contains("username") {
        return "张三".to_string();
    }
    if hint.contains("bool") || lower.starts_with("is_") || lower.starts_with("has_") {
        return "true".to_string();
    }
    if lower.contains("id")
        || lower.ends_with("no")
        || hint.contains("int")
        || hint.contains("number")
    {
        return (10001 + seq).to_string();
    }
    format!("示例值{}", seq + 1)
}
```

- [ ] **Step 7: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::masking -- --nocapture`
Expected: `sensitive_names_use_whole_words`、`secret_set_...`、`json_masking_...`、`userinfo_is_masked` 全部 PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::examples -- --nocapture`
Expected: `auto_values_are_deterministic` PASS

---

### Task 2: Markdown 渲染器

**Files:**
- Create: `src-tauri/src/docs/render_markdown.rs`

**Interfaces:**
- Consumes: `model::{DocBundle, DocGroup, DocRequest, DocBody, DocParam}`
- Produces: `render_markdown::render(bundle: &DocBundle) -> String`

- [ ] **Step 1: 写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::*;

    fn fixture() -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1.1.0".into(),
            revision: "r1".into(),
            subtitle: "测试副标题".into(),
            scope: DocScopeInfo {
                kind: "project".into(),
                title: "开放平台".into(),
                project_name: "开放平台".into(),
                target_id: 1,
                environment_id: None,
                environment_name: Some("测试环境".into()),
            },
            warnings: vec![],
            environments: vec![],
            groups: vec![DocGroup {
                id: 10,
                name: "用户 | 中心".into(),
                description: "".into(),
                path: vec!["用户 | 中心".into()],
                children: vec![],
                requests: vec![DocRequest {
                    id: 20,
                    name: "登录\n# 不能变标题".into(),
                    method: "POST".into(),
                    url: "https://api.example.com/login".into(),
                    path: "/login".into(),
                    description: "".into(),
                    query: vec![DocParam {
                        name: "user".into(),
                        value: "a|b".into(),
                        required: true,
                        location: "query".into(),
                        example_source: "defined".into(),
                    }],
                    path_params: vec![],
                    headers: vec![],
                    auth: DocAuth::default(),
                    body: DocBody::default(),
                    scripts: None,
                    assertions: None,
                }],
            }],
        }
    }

    #[test]
    fn escapes_table_pipes_and_heading_breaks() {
        let text = render(&fixture());
        assert!(text.contains("用户 \\| 中心"), "{text}");
        assert!(text.contains("a\\|b"), "{text}");
        assert!(text.contains("登录 # 不能变标题"), "{text}");
        assert!(!text.contains("登录\n#"), "{text}");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::render_markdown`
Expected: 编译失败（`render` 不存在）

- [ ] **Step 3: 写渲染器**

```rust
use super::model::{DocBody, DocBundle, DocGroup, DocParam, DocRequest};

const MASK: &str = "••••••";

fn title(text: &str) -> String {
    text.replace('\n', " ").replace('\r', " ").replace('#', "\\#")
}

fn cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "<br>")
        .replace('\r', "")
}

fn param_table(title: &str, params: &[DocParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "#### {title}\n\n| 名称 | 必填 | 示例 | 示例来源 |\n|---|---|---|---|\n"
    );
    for item in params {
        let value = if item.value.is_empty() { "—".to_string() } else { cell(&item.value) };
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            cell(&item.name),
            if item.required { "是" } else { "否" },
            value,
            cell(&item.example_source)
        ));
    }
    out.push('\n');
    out
}

fn body_section(body: &DocBody) -> String {
    if body.kind == "none" || body.text.trim().is_empty() {
        return String::new();
    }
    let lang = match body.kind.as_str() {
        "json" => "json",
        "graphql" => "graphql",
        "form" | "multipart" => "text",
        _ => "text",
    };
    format!(
        "#### 请求体（{}）\n\n````{lang}\n{}\n````\n\n",
        body.kind, body.text
    )
}

fn message(bundle: &DocBundle) -> String {
    let mut out = format!("# {}\n\n", title(&bundle.scope.title));
    if !bundle.subtitle.trim().is_empty() {
        out.push_str(&format!("> {}\n", title(&bundle.subtitle)));
    }
    out.push_str(&format!(
        "> 项目：{} · 生成日期：{}\n\n",
        title(&bundle.scope.project_name),
        chrono::Local::now().format("%Y-%m-%d")
    ));
    if let Some(env) = &bundle.scope.environment_name {
        out.push_str(&format!("> 环境：{env}（本文档地址已按该环境解析）\n\n"));
    }
    out
}

fn environments(bundle: &DocBundle) -> String {
    if bundle.environments.is_empty() {
        return String::new();
    }
    let mut out = String::from("## 环境变量\n\n");
    for env in &bundle.environments {
        out.push_str(&format!("### {}\n\n| 变量 | 值 |\n|---|---|\n", title(&env.name)));
        for item in &env.variables {
            out.push_str(&format!("| {} | {} |\n", cell(&item.key), cell(&item.value)));
        }
        out.push('\n');
    }
    out
}

fn request_section(request: &DocRequest, depth: usize) -> String {
    let level = (4 + depth).min(6);
    let mut out = format!(
        "{} {}\n\n- 方法：{}\n- 地址：{}\n- 路径：{}\n",
        "#".repeat(level),
        title(&request.name),
        request.method,
        request.url,
        request.path
    );
    if !request.description.trim().is_empty() && request.description != request.name {
        out.push_str(&format!("- 说明：{}\n", title(&request.description)));
    }
    if !request.auth.kind.is_empty() && request.auth.kind != "none" {
        out.push_str(&format!(
            "- 认证：{} · {}\n",
            request.auth.kind, title(&request.auth.note)
        ));
    }
    out.push('\n');
    out.push_str(&param_table("请求参数", &request.query));
    out.push_str(&param_table("路径参数", &request.path_params));
    out.push_str(&param_table("请求头", &request.headers));
    out.push_str(&body_section(&request.body));
    if let Some(scripts) = &request.scripts {
        if !scripts.pre.trim().is_empty() || !scripts.post.trim().is_empty() {
            out.push_str("#### 脚本\n\n");
            if !scripts.pre.trim().is_empty() {
                out.push_str(&format!("**前置脚本**\n\n````js\n{}\n````\n\n", scripts.pre));
            }
            if !scripts.post.trim().is_empty() {
                out.push_str(&format!("**后置脚本**\n\n````js\n{}\n````\n\n", scripts.post));
            }
        }
    }
    if let Some(assertions) = &request.assertions {
        if !assertions.trim().is_empty() {
            out.push_str(&format!("#### 断言\n\n````text\n{assertions}\n````\n\n"));
        }
    }
    out
}

fn group_section(group: &DocGroup, depth: usize) -> String {
    let level = (2 + depth).min(6);
    let mut out = format!("{} {}\n\n", "#".repeat(level), title(&group.name));
    if !group.description.trim().is_empty() && group.description != group.name {
        out.push_str(&format!("{}\n\n", title(&group.description)));
    }
    for request in &group.requests {
        out.push_str(&request_section(request, depth));
    }
    for child in &group.children {
        out.push_str(&group_section(child, depth + 1));
    }
    out
}

pub fn render(bundle: &DocBundle) -> String {
    let mut out = message(bundle);
    out.push_str(&environments(bundle));
    for group in &bundle.groups {
        out.push_str(&group_section(group, 0));
    }
    if bundle.groups.is_empty() {
        out.push_str("_该项目暂无接口_\n");
    }
    if !bundle.warnings.is_empty() {
        out.push_str("\n## 导出提示\n\n");
        for warning in &bundle.warnings {
            out.push_str(&format!("- {warning}\n"));
        }
    }
    out
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::render_markdown`
Expected: PASS

---

### Task 3: HTML 渲染器

**Files:**
- Create: `src-tauri/src/docs/render_html.rs`

**Interfaces:**
- Produces: `render_html::render(bundle: &DocBundle) -> String`

- [ ] **Step 1: 写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::*;

    fn fixture(name: &str) -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1.1.0".into(),
            revision: "r1".into(),
            subtitle: String::new(),
            scope: DocScopeInfo {
                kind: "request".into(),
                title: "<script>alert(1)</script>".into(),
                project_name: "项目".into(),
                target_id: 1,
                environment_id: None,
                environment_name: None,
            },
            warnings: vec![],
            environments: vec![],
            groups: vec![DocGroup {
                id: 1,
                name: "集合".into(),
                description: String::new(),
                path: vec!["集合".into()],
                children: vec![],
                requests: vec![DocRequest {
                    id: 2,
                    name: name.into(),
                    method: "GET".into(),
                    url: "https://example.com/a?x=1&y=2".into(),
                    path: "/a".into(),
                    description: String::new(),
                    query: vec![],
                    path_params: vec![],
                    headers: vec![],
                    auth: DocAuth::default(),
                    body: DocBody::default(),
                    scripts: None,
                    assertions: None,
                }],
            }],
        }
    }

    #[test]
    fn escapes_script_tags_and_has_no_javascript() {
        let html = render(&fixture("<script>alert(1)</script>"));
        assert!(!html.contains("<script"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("<!doctype html>"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::render_html`
Expected: 编译失败

- [ ] **Step 3: 写渲染器**

```rust
use super::model::{DocBody, DocBundle, DocGroup, DocParam, DocRequest};

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn table(title: &str, params: &[DocParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<h4>{}</h4><table><thead><tr><th>名称</th><th>必填</th><th>示例</th><th>来源</th></tr></thead><tbody>",
        esc(title)
    );
    for item in params {
        let value = if item.value.is_empty() { "—".to_string() } else { esc(&item.value) };
        out.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            esc(&item.name),
            if item.required { "是" } else { "否" },
            value,
            esc(&item.example_source)
        ));
    }
    out.push_str("</tbody></table>");
    out
}

fn body_block(body: &DocBody) -> String {
    if body.kind == "none" || body.text.trim().is_empty() {
        return String::new();
    }
    format!(
        "<h4>请求体（{}）</h4><pre><code>{}</code></pre>",
        esc(&body.kind),
        esc(&body.text)
    )
}

fn request(req: &DocRequest) -> String {
    let mut out = format!(
        "<details class=\"req\"><summary><span class=\"method\">{}</span> <span class=\"name\">{}</span> <code>{}</code></summary>",
        esc(&req.method),
        esc(&req.name),
        esc(&req.path)
    );
    out.push_str("<div class=\"meta\">");
    out.push_str(&format!("<div>地址：<code>{}</code></div>", esc(&req.url)));
    if !req.description.trim().is_empty() && req.description != req.name {
        out.push_str(&format!("<div>说明：{}</div>", esc(&req.description)));
    }
    if !req.auth.kind.is_empty() && req.auth.kind != "none" {
        out.push_str(&format!(
            "<div>认证：{} · {}</div>",
            esc(&req.auth.kind),
            esc(&req.auth.note)
        ));
    }
    out.push_str("</div>");
    out.push_str(&table("请求参数", &req.query));
    out.push_str(&table("路径参数", &req.path_params));
    out.push_str(&table("请求头", &req.headers));
    out.push_str(&body_block(&req.body));
    if let Some(scripts) = &req.scripts {
        if !scripts.pre.trim().is_empty() {
            out.push_str(&format!("<h4>前置脚本</h4><pre><code>{}</code></pre>", esc(&scripts.pre)));
        }
        if !scripts.post.trim().is_empty() {
            out.push_str(&format!("<h4>后置脚本</h4><pre><code>{}</code></pre>", esc(&scripts.post)));
        }
    }
    if let Some(assertions) = &req.assertions {
        if !assertions.trim().is_empty() {
            out.push_str(&format!("<h4>断言</h4><pre><code>{}</code></pre>", esc(assertions)));
        }
    }
    out.push_str("</details>");
    out
}

fn group(node: &DocGroup, depth: usize) -> String {
    let level = (2 + depth).min(6);
    let mut out = format!("<h{level}>{}</h{level}>", esc(&node.name));
    if !node.description.trim().is_empty() && node.description != node.name {
        out.push_str(&format!("<p class=\"desc\">{}</p>", esc(&node.description)));
    }
    for req in &node.requests {
        out.push_str(&request(req));
    }
    for child in &node.children {
        out.push_str(&group(child, depth + 1));
    }
    out
}

pub fn render(bundle: &DocBundle) -> String {
    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>", esc(&bundle.scope.title)));
    if !bundle.subtitle.trim().is_empty() {
        body.push_str(&format!("<p class=\"subtitle\">{}</p>", esc(&bundle.subtitle)));
    }
    body.push_str(&format!(
        "<p class=\"meta\">项目：{} · 生成日期：{}</p>",
        esc(&bundle.scope.project_name),
        chrono::Local::now().format("%Y-%m-%d")
    ));
    if let Some(env) = &bundle.scope.environment_name {
        body.push_str(&format!(
            "<p class=\"meta\">环境：{}（本文档地址已按该环境解析）</p>",
            esc(env)
        ));
    }
    if !bundle.environments.is_empty() {
        body.push_str("<h2>环境变量</h2>");
        for env in &bundle.environments {
            body.push_str(&format!("<h3>{}</h3><table><thead><tr><th>变量</th><th>值</th></tr></thead><tbody>", esc(&env.name)));
            for item in &env.variables {
                body.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", esc(&item.key), esc(&item.value)));
            }
            body.push_str("</tbody></table>");
        }
    }
    for node in &bundle.groups {
        body.push_str(&group(node, 0));
    }
    if bundle.groups.is_empty() {
        body.push_str("<p class=\"empty\">该项目暂无接口</p>");
    }
    if !bundle.warnings.is_empty() {
        body.push_str("<h2>导出提示</h2><ul>");
        for warning in &bundle.warnings {
            body.push_str(&format!("<li>{}</li>", esc(warning)));
        }
        body.push_str("</ul>");
    }

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">
<title>{title}</title>
<style>
  :root {{ color-scheme: light; }}
  body {{ margin: 0 auto; max-width: 980px; padding: 32px 24px 80px; font: 14px/1.7 -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif; color: #1f2329; background: #fff; }}
  h1 {{ font-size: 28px; margin: 0 0 8px; }}
  h2 {{ font-size: 22px; border-bottom: 1px solid #e5e6eb; padding-bottom: 6px; margin-top: 36px; }}
  h3, h4 {{ margin: 22px 0 8px; }}
  .subtitle, .meta, .desc {{ color: #646a73; }}
  .req {{ border: 1px solid #e5e6eb; border-radius: 6px; margin: 14px 0; padding: 10px 14px; page-break-inside: avoid; }}
  .req summary {{ cursor: pointer; }}
  .method {{ display: inline-block; min-width: 52px; color: #009688; font-weight: 700; }}
  .name {{ font-weight: 600; }}
  table {{ border-collapse: collapse; width: 100%; margin: 10px 0; }}
  th, td {{ border: 1px solid #e5e6eb; padding: 6px 8px; text-align: left; vertical-align: top; }}
  th {{ background: #f7f8fa; }}
  code, pre {{ font-family: Consolas, "Courier New", monospace; }}
  pre {{ padding: 12px; background: #f7f8fa; border: 1px solid #e5e6eb; overflow: auto; white-space: pre-wrap; word-break: break-word; }}
  @page {{ size: A4; margin: 16mm; }}
  @media print {{ body {{ max-width: none; padding: 0; }} .req {{ break-inside: avoid; }} }}
</style>
</head>
<body>
{body}
</body>
</html>"#,
        title = esc(&bundle.scope.title),
        body = body
    )
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::render_html`
Expected: PASS

---

### Task 4: builder（读库、环境解析、示例链、IR 组装）

**Files:**
- Create: `src-tauri/src/docs/builder.rs`
- Modify: `src-tauri/src/commands/request.rs:22`（`fn map_detail` → `pub(crate) fn map_detail`）

**Interfaces:**
- Consumes: `docs::model::*`、`docs::masking::*`、`docs::examples::auto_value`、`commands::request::map_detail`
- Produces:
  - `builder::build(conn: &rusqlite::Connection, payload: &DocsPreparePayload) -> Result<DocBundle, String>`
  - `builder::outline(bundle: &DocBundle) -> DocOutline`
  - `builder::request_count(bundle: &DocBundle) -> usize`
  - `builder::collection_count(bundle: &DocBundle) -> usize`

- [ ] **Step 1: 写集成测试（builder.rs 底部）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;

    fn pool(tag: &str) -> crate::state::DbPool {
        let dir = crate::util::test_dir(tag);
        let manager = SqliteConnectionManager::file(dir.join("astroman.db"));
        let pool = r2d2::Pool::builder().max_size(2).build(manager).unwrap();
        db::init(&pool).unwrap();
        pool
    }

    fn seed(conn: &rusqlite::Connection) {
        conn.execute(
            "INSERT INTO projects (name, description, color, sort_order, created_at, updated_at) VALUES ('P','',NULL,0,0,0)",
            [],
        )
        .unwrap();
        let project = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO collections (project_id, parent_id, name, sort_order, created_at, updated_at) VALUES (?1,NULL,'C',0,0,0)",
            params![project],
        )
        .unwrap();
        let collection = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO requests (collection_id,name,method,url,headers,params,body_type,body,body_meta,pre_script,post_script,assertions,auth,sort_order,created_at,updated_at)
             VALUES (?1,'登录','POST','https://{{host}}/login?debug=1','[]','[{\"key\":\"page\",\"value\":\"\",\"enabled\":true}]','json','{\"username\":\"u\",\"password\":\"p\"}','','','','','{\"kind\":\"none\"}',0,0,0)",
            params![collection],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO environments (project_id,name,is_active,script,created_at) VALUES (?1,'测试',1,'',0)",
            params![project],
        )
        .unwrap();
        let env = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO env_variables (env_id,\"key\",value,is_secret,sort_order) VALUES (?1,'host','api.example.com',0,0), (?1,'api_token','tok-secret',1,1)",
            params![env],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_runs (request_id,method,url,headers,params,body_type,body,response_status,created_at,origin)
             VALUES (?1,'POST','https://api.example.com/login?debug=1','[]','[{\"key\":\"page\",\"value\":\"2\",\"enabled\":true}]','json','{\"username\":\"run-user\"}',200,?2,'manual')",
            params![conn.query_row("SELECT id FROM requests LIMIT 1", [], |r| r.get::<_, i64>(0)).unwrap(), crate::util::now_ms()],
        )
        .unwrap();
    }

    #[test]
    fn builds_project_bundle_with_env_and_run_examples() {
        let pool = pool("docs-builder");
        let conn = pool.get().unwrap();
        seed(&conn);
        let project_id: i64 = conn.query_row("SELECT id FROM projects LIMIT 1", [], |r| r.get(0)).unwrap();
        let env_id: i64 = conn.query_row("SELECT id FROM environments LIMIT 1", [], |r| r.get(0)).unwrap();
        let payload = DocsPreparePayload {
            scope: "project".into(),
            target_id: project_id,
            environment_id: Some(env_id),
            options: DocsOptions::default(),
        };
        let bundle = build(&conn, &payload).unwrap();
        let request = &bundle.groups[0].requests[0];
        assert_eq!(bundle.scope.environment_name.as_deref(), Some("测试"));
        assert!(request.url.contains("api.example.com"));
        assert_eq!(request.query[0].value, "2");
        assert_eq!(request.query[0].example_source, "run");
        assert_eq!(request.body.fields[0].name, "username");
        assert_eq!(bundle.environments[0].variables[1].value, "••••••");
        assert!(!serde_json::to_string(&bundle).unwrap().contains("tok-secret"));
        let outline = outline(&bundle);
        assert_eq!(outline.groups[0].requests[0].key, format!("r-{}", request.id));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::builder`
Expected: 编译失败（`build` / `outline` 不存在）

- [ ] **Step 3: 把 `map_detail` 改成 `pub(crate)`**

`src-tauri/src/commands/request.rs` 第 24 行：

```rust
pub(crate) fn map_detail(row: &Row<'_>) -> rusqlite::Result<HttpRequestDetail> {
```

- [ ] **Step 4: 写 `builder.rs`**

```rust
use crate::commands::request::map_detail;
use crate::models::{HttpRequestDetail, KeyValueItem};
use crate::secret;
use crate::util::{collect_placeholders, err, now_ms, render_tracked};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

use super::examples;
use super::masking::{self, SecretSet};
use super::model::*;

pub const MAX_REQUESTS: usize = 5000;
pub const MAX_BODY_BYTES: usize = 64 * 1024;

struct CollectionRow {
    id: i64,
    parent_id: Option<i64>,
    name: String,
}

struct RequestRow {
    id: i64,
    collection_id: i64,
    detail: HttpRequestDetail,
}

struct EnvData {
    id: i64,
    name: String,
    vars: HashMap<String, String>,
    secrets: Vec<String>,
    variables: Vec<DocEnvVar>,
}

struct RunData {
    url: String,
    params: Vec<KeyValueItem>,
    body: String,
    body_type: String,
    truncated: bool,
}

struct BuildCtx<'a> {
    env: Option<&'a EnvData>,
    runs: &'a HashMap<i64, RunData>,
    options: &'a DocsOptions,
    secrets: &'a mut SecretSet,
    unresolved: usize,
}

struct ScopeCtx {
    kind: String,
    project_id: i64,
    project_name: String,
    title: String,
    collection_id: Option<i64>,
}

pub fn build(conn: &Connection, payload: &DocsPreparePayload) -> Result<DocBundle, String> {
    let scope = resolve_scope(conn, payload)?;
    let collections = load_collections(conn, scope.project_id)?;
    let requests = load_requests(conn, scope.project_id)?;
    if requests.len() > MAX_REQUESTS {
        return Err(format!(
            "E_TOO_LARGE: 本次导出包含 {} 个请求，超过 {} 个上限，请按集合分批导出",
            requests.len(),
            MAX_REQUESTS
        ));
    }

    let env = load_environment(conn, scope.project_id, payload.environment_id)?;
    let locked = secret::store().map(|store| store.is_locked()).unwrap_or(false);
    if locked
        && (env.as_ref().map(|item| !item.secrets.is_empty()).unwrap_or(false)
            || requests.iter().any(|row| {
                !row.detail.auth.kind.is_empty() && row.detail.auth.kind != "none"
            }))
    {
        return Err("E_SECRET_LOCKED: 主密钥未解锁，请先在设置页解锁后再导出".to_string());
    }

    let runs = if env.is_some() && payload.options.use_run_examples {
        load_runs(conn, scope.project_id, payload.options.run_examples_max_age_days)?
    } else {
        HashMap::new()
    };

    let mut secrets = SecretSet::default();
    if let Some(env) = &env {
        secrets.extend(env.secrets.iter().cloned());
    }
    let mut ctx = BuildCtx {
        env: env.as_ref(),
        runs: &runs,
        options: &payload.options,
        secrets: &mut secrets,
        unresolved: 0,
    };

    let children = children_map(&collections);
    let requests_by_collection = requests_map(&requests);
    let mut groups = Vec::new();
    let keep_empty: HashSet<i64> = match scope.collection_id {
        Some(id) => [id].into_iter().collect(),
        None => HashSet::new(),
    };
    let roots: Vec<i64> = match scope.collection_id {
        Some(id) => vec![id],
        None => collections
            .iter()
            .filter(|item| item.parent_id.is_none())
            .map(|item| item.id)
            .collect(),
    };
    for id in roots {
        if let Some(group) = build_group(
            id,
            &collections,
            &children,
            &requests_by_collection,
            &[],
            &keep_empty,
            &mut ctx,
        )? {
            groups.push(group);
        }
    }

    for group in &mut groups {
        mask_group(group, ctx.secrets);
    }

    let environments = match env.as_ref() {
        Some(env) if payload.options.include_environment => vec![DocEnvironment {
            name: env.name.clone(),
            is_active: true,
            variables: env.variables.clone(),
        }],
        _ => Vec::new(),
    };

    let mut warnings = Vec::new();
    if ctx.unresolved > 0 {
        warnings.push(format!("{} 处变量未解析，已保留占位符", ctx.unresolved));
    }

    Ok(DocBundle {
        generated_at: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        revision: revision_of(&collections, &requests),
        subtitle: String::new(),
        scope: DocScopeInfo {
            kind: scope.kind,
            title: scope.title,
            project_name: scope.project_name,
            target_id: payload.target_id,
            environment_id: env.as_ref().map(|item| item.id),
            environment_name: env.as_ref().map(|item| item.name.clone()),
        },
        groups,
        environments,
        warnings,
    })
}

fn resolve_scope(conn: &Connection, payload: &DocsPreparePayload) -> Result<ScopeCtx, String> {
    match payload.scope.as_str() {
        "request" => {
            let row = conn
                .query_row(
                    "SELECT r.name, r.collection_id, c.project_id, p.name
                     FROM requests r
                     JOIN collections c ON c.id = r.collection_id
                     JOIN projects p ON p.id = c.project_id
                     WHERE r.id = ?1",
                    params![payload.target_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 请求不存在".to_string(),
                    other => err(other),
                })?;
            Ok(ScopeCtx {
                kind: "request".into(),
                project_id: row.2,
                project_name: row.3,
                title: row.0,
                collection_id: Some(row.1),
            })
        }
        "collection" => {
            let row = conn
                .query_row(
                    "SELECT c.name, c.project_id, p.name
                     FROM collections c JOIN projects p ON p.id = c.project_id
                     WHERE c.id = ?1",
                    params![payload.target_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 集合不存在".to_string(),
                    other => err(other),
                })?;
            Ok(ScopeCtx {
                kind: "collection".into(),
                project_id: row.1,
                project_name: row.2,
                title: row.0,
                collection_id: Some(payload.target_id),
            })
        }
        "project" => {
            let name = conn
                .query_row(
                    "SELECT name FROM projects WHERE id = ?1",
                    params![payload.target_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 项目不存在".to_string(),
                    other => err(other),
                })?;
            Ok(ScopeCtx {
                kind: "project".into(),
                project_id: payload.target_id,
                project_name: name.clone(),
                title: name,
                collection_id: None,
            })
        }
        _ => Err("E_SCOPE: 不支持的导出范围".to_string()),
    }
}

fn load_collections(conn: &Connection, project_id: i64) -> Result<Vec<CollectionRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, name FROM collections
             WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(CollectionRow {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn load_requests(conn: &Connection, project_id: i64) -> Result<Vec<RequestRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.collection_id, r.name, r.method, r.url, r.headers, r.params,
                    r.body_type, r.body, r.pre_script, r.post_script, r.assertions, r.auth,
                    r.body_meta, r.sort_order
             FROM requests r JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1 ORDER BY r.sort_order ASC, r.id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(RequestRow {
                id: row.get(0)?,
                collection_id: row.get(1)?,
                detail: map_detail(row)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

fn load_environment(
    conn: &Connection,
    project_id: i64,
    environment_id: Option<i64>,
) -> Result<Option<EnvData>, String> {
    let Some(environment_id) = environment_id else {
        return Ok(None);
    };
    let name = conn
        .query_row(
            "SELECT name FROM environments WHERE id = ?1 AND project_id = ?2",
            params![environment_id, project_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => "E_SCOPE: 环境不存在或不属于当前项目".to_string(),
            other => err(other),
        })?;
    let mut stmt = conn
        .prepare(
            "SELECT \"key\", value, is_secret FROM env_variables
             WHERE env_id = ?1 ORDER BY sort_order ASC, id ASC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![environment_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
            ))
        })
        .map_err(err)?;

    let mut vars = HashMap::new();
    let mut secrets = Vec::new();
    let mut variables = Vec::new();
    for item in rows {
        let (key, raw, is_secret) = item.map_err(err)?;
        if key.trim().is_empty() {
            continue;
        }
        let plain = if is_secret {
            secret::decrypt(secret::AAD_ENV, &raw)
        } else {
            raw
        };
        if is_secret && !plain.is_empty() {
            secrets.push(plain.clone());
        }
        variables.push(DocEnvVar {
            key: key.clone(),
            value: if is_secret && !plain.is_empty() { MASK.to_string() } else { plain.clone() },
            is_secret,
        });
        vars.insert(key, plain);
    }
    Ok(Some(EnvData {
        id: environment_id,
        name,
        vars,
        secrets,
        variables,
    }))
}

fn load_runs(
    conn: &Connection,
    project_id: i64,
    max_age_days: u32,
) -> Result<HashMap<i64, RunData>, String> {
    let recent = if max_age_days == 0 {
        String::new()
    } else {
        format!(
            " AND rr.created_at >= {}",
            now_ms() - i64::from(max_age_days) * 86_400_000
        )
    };
    let sql = format!(
        "SELECT request_id, url, params, body, body_type, request_body_truncated
         FROM (
             SELECT rr.request_id, rr.url, rr.params, rr.body, rr.body_type,
                    rr.request_body_truncated,
                    ROW_NUMBER() OVER (
                        PARTITION BY rr.request_id ORDER BY rr.created_at DESC, rr.id DESC
                    ) AS rn
             FROM request_runs rr
             JOIN requests r ON r.id = rr.request_id
             JOIN collections c ON c.id = r.collection_id
             WHERE c.project_id = ?1{recent}
         ) WHERE rn = 1"
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                RunData {
                    url: row.get::<_, String>(1)?,
                    params: parse_items(row.get::<_, Option<String>>(2)?),
                    body: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    body_type: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    truncated: row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                },
            ))
        })
        .map_err(err)?;
    let mut result = HashMap::new();
    for item in rows {
        let (request_id, run) = item.map_err(err)?;
        result.insert(request_id, run);
    }
    Ok(result)
}

fn parse_items(raw: Option<String>) -> Vec<KeyValueItem> {
    raw.and_then(|text| serde_json::from_str::<Vec<KeyValueItem>>(&text).ok())
        .unwrap_or_default()
}

fn children_map(collections: &[CollectionRow]) -> HashMap<Option<i64>, Vec<i64>> {
    let mut map: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
    for item in collections {
        map.entry(item.parent_id).or_default().push(item.id);
    }
    map
}

fn requests_map(requests: &[RequestRow]) -> HashMap<i64, Vec<usize>> {
    let mut map: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, item) in requests.iter().enumerate() {
        map.entry(item.collection_id).or_default().push(index);
    }
    map
}

#[allow(clippy::too_many_arguments)]
fn build_group(
    collection_id: i64,
    collections: &[CollectionRow],
    children: &HashMap<Option<i64>, Vec<i64>>,
    requests: &HashMap<i64, Vec<usize>>,
    ancestors: &[String],
    keep_empty: &HashSet<i64>,
    ctx: &mut BuildCtx<'_>,
) -> Result<Option<DocGroup>, String> {
    let Some(collection) = collections.iter().find(|item| item.id == collection_id) else {
        return Ok(None);
    };
    let mut path = ancestors.to_vec();
    path.push(collection.name.clone());
    let mut doc_requests = Vec::new();
    if let Some(indexes) = requests.get(&collection_id) {
        for index in indexes {
            doc_requests.push(build_request(&requests_all(ctx)[*index], ctx)?);
        }
    }
    let mut doc_children = Vec::new();
    if let Some(ids) = children.get(&Some(collection_id)) {
        for id in ids {
            if let Some(group) =
                build_group(*id, collections, children, requests, &path, keep_empty, ctx)?
            {
                doc_children.push(group);
            }
        }
    }
    if doc_requests.is_empty() && doc_children.is_empty() && !keep_empty.contains(&collection_id) {
        return Ok(None);
    }
    Ok(Some(DocGroup {
        id: collection_id,
        name: collection.name.clone(),
        description: collection.name.clone(),
        path,
        requests: doc_requests,
        children: doc_children,
    }))
}
```

> 说明：`requests_all(ctx)` 不存在。为避免在递归里再传一份 `&[RequestRow]`，把 `build_group` 多加一个参数 `all_requests: &[RequestRow]`，用 `&all_requests[*index]`。修改签名与调用处即可（`build` 里传入 `&requests`，递归原样传递）。

继续补全同一文件：

```rust
fn build_request(row: &RequestRow, ctx: &mut BuildCtx<'_>) -> Result<DocRequest, String> {
    let detail = &row.detail;
    let mut seq = 0usize;

    let (raw_url, _) = resolve_text(&detail.url, ctx);
    let path = path_of(&detail.url);

    let mut query = Vec::new();
    for item in detail.params.iter().filter(|item| item.enabled && !item.key.trim().is_empty()) {
        let sensitive = masking::is_sensitive_name(&item.key);
        let (defined, _) = resolve_text(&item.value, ctx);
        let run_value = ctx.runs.get(&row.id).and_then(|run| {
            run.params
                .iter()
                .find(|param| param.enabled && param.key == item.key)
                .map(|param| param.value.clone())
                .filter(|value| !value.is_empty())
        });
        let (value, source) = if sensitive {
            (MASK.to_string(), "defined".to_string())
        } else if !defined.trim().is_empty() && !defined.contains("{{") {
            (defined, "defined".to_string())
        } else if let Some(value) = run_value {
            (value, "run".to_string())
        } else if ctx.options.auto_examples {
            (examples::auto_value(&item.key, "", seq, row.id), "auto".to_string())
        } else {
            (String::new(), "defined".to_string())
        };
        seq += 1;
        query.push(DocParam {
            name: item.key.clone(),
            value,
            required: false,
            location: "query".to_string(),
            example_source: source,
        });
    }

    let mut path_params = Vec::new();
    for (index, name) in collect_placeholders(&detail.url).iter().enumerate() {
        let sensitive = masking::is_sensitive_name(name);
        let run_value = ctx
            .runs
            .get(&row.id)
            .and_then(|run| path_segment(&detail.url, &run.url, name));
        let (value, source) = if sensitive {
            (MASK.to_string(), "defined".to_string())
        } else if let Some(value) = run_value {
            (value, "run".to_string())
        } else if ctx.options.auto_examples {
            (examples::auto_value(name, "", index, row.id), "auto".to_string())
        } else {
            (String::new(), "defined".to_string())
        };
        path_params.push(DocParam {
            name: name.clone(),
            value,
            required: true,
            location: "path".to_string(),
            example_source: source,
        });
    }

    let mut headers = Vec::new();
    for item in detail.headers.iter().filter(|item| item.enabled && !item.key.trim().is_empty()) {
        let lower = item.key.to_ascii_lowercase();
        if matches!(lower.as_str(), "authorization" | "host" | "content-length") {
            continue;
        }
        let sensitive = masking::is_sensitive_name(&item.key);
        let (defined, _) = resolve_text(&item.value, ctx);
        headers.push(DocParam {
            name: item.key.clone(),
            value: if sensitive { MASK.to_string() } else { defined },
            required: false,
            location: "header".to_string(),
            example_source: "defined".to_string(),
        });
    }

    let auth = build_auth(&detail.auth, ctx.secrets);
    let body = build_body(detail, row.id, ctx)?;

    let scripts = if ctx.options.include_scripts
        && (!detail.pre_script.trim().is_empty() || !detail.post_script.trim().is_empty())
    {
        Some(DocScripts {
            pre: detail.pre_script.clone(),
            post: detail.post_script.clone(),
        })
    } else {
        None
    };
    let assertions = if ctx.options.include_assertions && !detail.assertions.trim().is_empty() {
        Some(detail.assertions.clone())
    } else {
        None
    };

    Ok(DocRequest {
        id: row.id,
        name: detail.name.clone(),
        method: detail.method.to_uppercase(),
        url: masking::mask_userinfo(&raw_url),
        path,
        description: detail.name.clone(),
        query,
        path_params,
        headers,
        auth,
        body,
        scripts,
        assertions,
    })
}
```

同样补全辅助函数：

```rust
fn resolve_text(input: &str, ctx: &mut BuildCtx<'_>) -> (String, bool) {
    if !ctx.options.resolve_environment {
        return (input.to_string(), input.contains("{{"));
    }
    let Some(env) = ctx.env else {
        return (input.to_string(), input.contains("{{"));
    };
    let mut used = Vec::new();
    let out = render_tracked(input, &env.vars, &mut used);
    for value in used {
        ctx.secrets.add(&value);
    }
    let unresolved = out.contains("{{");
    if unresolved {
        ctx.unresolved += 1;
    }
    (out, unresolved)
}

fn build_auth(auth: &crate::models::RequestAuth, secrets: &mut SecretSet) -> DocAuth {
    match auth.kind.as_str() {
        "bearer" => {
            secrets.add(&auth.token);
            DocAuth {
                kind: "bearer".into(),
                add_to: auth.add_to.clone(),
                key_name: "Authorization".into(),
                note: "Bearer Token（值已隐藏）".into(),
            }
        }
        "basic" => {
            secrets.add(&auth.username);
            secrets.add(&auth.password);
            DocAuth {
                kind: "basic".into(),
                add_to: auth.add_to.clone(),
                key_name: "Authorization".into(),
                note: "Basic（用户名与口令已隐藏）".into(),
            }
        }
        "apikey" => {
            secrets.add(&auth.value);
            DocAuth {
                kind: "apikey".into(),
                add_to: if auth.add_to.is_empty() { "header".into() } else { auth.add_to.clone() },
                key_name: auth.key.clone(),
                note: "API Key（值已隐藏）".into(),
            }
        }
        "oauth2" => {
            secrets.add(&auth.client_secret);
            DocAuth {
                kind: "oauth2".into(),
                add_to: "header".into(),
                key_name: "Authorization".into(),
                note: format!(
                    "OAuth2 Client Credentials · clientId={} · tokenUrl={} · scope={}",
                    auth.client_id, auth.token_url, auth.scope
                ),
            }
        }
        _ => DocAuth::default(),
    }
}

fn build_body(
    detail: &HttpRequestDetail,
    request_id: i64,
    ctx: &mut BuildCtx<'_>,
) -> Result<DocBody, String> {
    let kind = detail.body_type.as_str();
    if kind == "none" {
        return Ok(DocBody::default());
    }
    let (resolved, _) = resolve_text(&detail.body, ctx);
    match kind {
        "json" => build_json_body(&resolved, request_id, ctx),
        "form" => {
            let mut fields = Vec::new();
            for (index, pair) in resolved.split('&').enumerate() {
                if pair.is_empty() {
                    continue;
                }
                let mut parts = pair.splitn(2, '=');
                let key = parts.next().unwrap_or_default();
                let value = parts.next().unwrap_or_default();
                if key.trim().is_empty() {
                    continue;
                }
                let sensitive = masking::is_sensitive_name(key);
                fields.push(DocParam {
                    name: key.to_string(),
                    value: if sensitive {
                        MASK.to_string()
                    } else if !value.is_empty() {
                        value.to_string()
                    } else if ctx.options.auto_examples {
                        examples::auto_value(key, "", index, request_id)
                    } else {
                        String::new()
                    },
                    required: false,
                    location: "form".into(),
                    example_source: "defined".into(),
                });
            }
            Ok(DocBody {
                kind: "form".into(),
                text: masking::truncate_body(&resolved, MAX_BODY_BYTES),
                fields,
            })
        }
        "multipart" => {
            let items = parse_items(Some(resolved.clone()));
            let mut fields = Vec::new();
            for (index, item) in items.iter().filter(|item| item.enabled).enumerate() {
                if item.key.trim().is_empty() {
                    continue;
                }
                let sensitive = masking::is_sensitive_name(&item.key);
                fields.push(DocParam {
                    name: item.key.clone(),
                    value: if sensitive {
                        MASK.to_string()
                    } else if !item.value.is_empty() {
                        item.value.clone()
                    } else if ctx.options.auto_examples {
                        examples::auto_value(&item.key, "", index, request_id)
                    } else {
                        String::new()
                    },
                    required: false,
                    location: "multipart".into(),
                    example_source: "defined".into(),
                });
            }
            Ok(DocBody {
                kind: "multipart".into(),
                text: masking::truncate_body(&resolved, MAX_BODY_BYTES),
                fields,
            })
        }
        _ => Ok(DocBody {
            kind: kind.to_string(),
            text: masking::truncate_body(&resolved, MAX_BODY_BYTES),
            fields: Vec::new(),
        }),
    }
}

fn build_json_body(
    raw: &str,
    request_id: i64,
    ctx: &mut BuildCtx<'_>,
) -> Result<DocBody, String> {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Ok(DocBody {
            kind: "json".into(),
            text: masking::truncate_body(raw, MAX_BODY_BYTES),
            fields: Vec::new(),
        });
    };
    let run_body = ctx
        .runs
        .get(&request_id)
        .filter(|run| run.body_type == "json")
        .and_then(|run| serde_json::from_str::<serde_json::Value>(&run.body).ok());
    let mut fields = Vec::new();
    if let serde_json::Value::Object(map) = &mut value {
        for (index, (key, item)) in map.iter_mut().enumerate() {
            let sensitive = masking::is_sensitive_name(key);
            if sensitive {
                *item = serde_json::Value::String(MASK.to_string());
                fields.push(DocParam {
                    name: key.clone(),
                    value: MASK.to_string(),
                    required: false,
                    location: "body".into(),
                    example_source: "defined".into(),
                });
                continue;
            }
            let defined = match item {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            let run_value = run_body
                .as_ref()
                .and_then(|value| value.get(key))
                .map(|item| match item {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .filter(|value| !value.is_empty());
            let (text, source) = if !defined.trim().is_empty() && !defined.contains("{{") {
                (defined, "defined")
            } else if let Some(value) = run_value {
                (value, "run")
            } else if ctx.options.auto_examples {
                (examples::auto_value(key, "", index, request_id), "auto")
            } else {
                (String::new(), "defined")
            };
            *item = serde_json::Value::String(text.clone());
            fields.push(DocParam {
                name: key.clone(),
                value: text,
                required: false,
                location: "body".into(),
                example_source: source.into(),
            });
        }
    }
    Ok(DocBody {
        kind: "json".into(),
        text: masking::truncate_body(
            &serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string()),
            MAX_BODY_BYTES,
        ),
        fields,
    })
}
```

继续补全掩码、大纲与计数：

```rust
fn mask_group(group: &mut DocGroup, secrets: &SecretSet) {
    group.name = secrets.mask_text(&group.name);
    group.description = secrets.mask_text(&group.description);
    for request in &mut group.requests {
        mask_request(request, secrets);
    }
    for child in &mut group.children {
        mask_group(child, secrets);
    }
}

fn mask_param(param: &mut DocParam, secrets: &SecretSet) {
    if masking::is_sensitive_name(&param.name) {
        param.value = MASK.to_string();
        return;
    }
    param.value = secrets.mask_text(&param.value);
}

fn mask_request(request: &mut DocRequest, secrets: &SecretSet) {
    request.name = secrets.mask_text(&request.name);
    request.description = secrets.mask_text(&request.description);
    request.url = secrets.mask_text(&request.url);
    request.path = secrets.mask_text(&request.path);
    for param in request
        .query
        .iter_mut()
        .chain(request.path_params.iter_mut())
        .chain(request.headers.iter_mut())
    {
        mask_param(param, secrets);
    }
    for param in &mut request.body.fields {
        mask_param(param, secrets);
    }
    request.body.text = if request.body.kind == "json" {
        secrets.mask_json(&request.body.text)
    } else {
        secrets.mask_text(&request.body.text)
    };
    if let Some(scripts) = &mut request.scripts {
        scripts.pre = secrets.mask_text(&scripts.pre);
        scripts.post = secrets.mask_text(&scripts.post);
    }
    if let Some(assertions) = &mut request.assertions {
        *assertions = secrets.mask_text(assertions);
    }
}

pub fn outline(bundle: &DocBundle) -> DocOutline {
    DocOutline {
        groups: bundle.groups.iter().map(outline_group).collect(),
    }
}

fn outline_group(group: &DocGroup) -> DocOutlineGroup {
    DocOutlineGroup {
        key: format!("g-{}", group.id),
        name: group.name.clone(),
        path: group.path.clone(),
        requests: group.requests.iter().map(outline_request).collect(),
        children: group.children.iter().map(outline_group).collect(),
    }
}

fn outline_request(request: &DocRequest) -> DocOutlineRequest {
    let mut params = Vec::new();
    push_fields(&mut params, &request.query, request.id, "query", "p");
    push_fields(&mut params, &request.path_params, request.id, "path", "p");
    push_fields(&mut params, &request.headers, request.id, "header", "p");
    let mut seen = HashMap::new();
    for item in &request.body.fields {
        let occurrence = seen.entry(item.name.clone()).or_insert(0usize);
        *occurrence += 1;
        let key = match request.body.kind.as_str() {
            "json" => format!("b-{}-{}", request.id, pointer_of(&item.name)),
            _ => format!("f-{}-{}-{}", request.id, item.name, occurrence),
        };
        params.push(DocOutlineField {
            key,
            name: item.name.clone(),
            location: "body".into(),
            value: item.value.clone(),
            source: item.example_source.clone(),
            sensitive: masking::is_sensitive_name(&item.name),
        });
    }
    DocOutlineRequest {
        key: format!("r-{}", request.id),
        name: request.name.clone(),
        method: request.method.clone(),
        url: request.url.clone(),
        description: request.description.clone(),
        included: true,
        params,
        warnings: Vec::new(),
    }
}

fn push_fields(
    out: &mut Vec<DocOutlineField>,
    list: &[DocParam],
    request_id: i64,
    location: &str,
    prefix: &str,
) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in list {
        let occurrence = seen.entry(item.name.clone()).or_insert(0usize);
        *occurrence += 1;
        out.push(DocOutlineField {
            key: format!("{prefix}-{request_id}-{location}-{}-{occurrence}", item.name),
            name: item.name.clone(),
            location: location.to_string(),
            value: item.value.clone(),
            source: item.example_source.clone(),
            sensitive: masking::is_sensitive_name(&item.name),
        });
    }
}

pub(crate) fn pointer_of(name: &str) -> String {
    format!(
        "/{}",
        name.replace('~', "~0").replace('/', "~1")
    )
}

fn path_of(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    if let Some(index) = after_scheme.find('/') {
        after_scheme[index..]
            .split(['?', '#'])
            .next()
            .unwrap_or("/")
            .to_string()
    } else {
        "/".to_string()
    }
}

fn path_segment(template: &str, actual: &str, name: &str) -> Option<String> {
    let left: Vec<&str> = path_of(template).split('/').collect();
    let right: Vec<&str> = path_of(actual).split('/').collect();
    if left.len() != right.len() {
        return None;
    }
    let needle = format!("{{{{{name}}}}}");
    left.iter()
        .position(|segment| segment.contains(&needle))
        .map(|index| right[index].to_string())
}

fn revision_of(collections: &[CollectionRow], requests: &[RequestRow]) -> String {
    let max_collection = collections.iter().map(|item| item.id).max().unwrap_or(0);
    let max_request = requests.iter().map(|item| item.id).max().unwrap_or(0);
    format!(
        "{}-{}-{}-{}",
        collections.len(),
        requests.len(),
        max_collection,
        max_request
    )
}

pub fn request_count(bundle: &DocBundle) -> usize {
    fn count_group(group: &DocGroup) -> usize {
        group.requests.len() + group.children.iter().map(count_group).sum::<usize>()
    }
    bundle.groups.iter().map(count_group).sum()
}

pub fn collection_count(bundle: &DocBundle) -> usize {
    fn count_group(group: &DocGroup) -> usize {
        1 + group.children.iter().map(count_group).sum::<usize>()
    }
    bundle.groups.iter().map(count_group).sum()
}
```

- [ ] **Step 5: 修正 `build_group` 签名**

把 `build_group` 增加参数 `all_requests: &[RequestRow]`，并把 `requests_all(ctx)[*index]` 改成 `&all_requests[*index]`；`build` 里首次调用传 `&requests`，递归调用同样传 `all_requests`。

- [ ] **Step 6: 运行 builder 测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::builder -- --nocapture`
Expected: `builds_project_bundle_with_env_and_run_examples` PASS；若 `tok-secret` 出现在序列化结果中，检查 `load_environment` 是否在 `variables` 里写入了明文。

---

### Task 5: 大纲覆盖层、缓存与 IR 合并

**Files:**
- Create: `src-tauri/src/docs/overlay.rs`
- Create: `src-tauri/src/docs/cache.rs`

**Interfaces:**
- Produces:
  - `overlay::apply(bundle: &mut DocBundle, overlay: &DocEditOverlay) -> Vec<String>`
  - `cache::DocsCache::{new, put, get}`

- [ ] **Step 1: 写测试**

```rust
// overlay.rs 底部
#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::*;

    fn bundle() -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1".into(),
            revision: "rev1".into(),
            subtitle: String::new(),
            scope: DocScopeInfo {
                kind: "project".into(),
                title: "旧标题".into(),
                project_name: "P".into(),
                target_id: 1,
                environment_id: None,
                environment_name: None,
            },
            warnings: vec![],
            environments: vec![],
            groups: vec![DocGroup {
                id: 1,
                name: "集合".into(),
                description: "集合".into(),
                path: vec!["集合".into()],
                children: vec![],
                requests: vec![DocRequest {
                    id: 2,
                    name: "接口".into(),
                    method: "GET".into(),
                    url: "https://example.com".into(),
                    path: "/".into(),
                    description: "接口".into(),
                    query: vec![DocParam {
                        name: "page".into(),
                        value: "1".into(),
                        required: false,
                        location: "query".into(),
                        example_source: "auto".into(),
                    }],
                    path_params: vec![],
                    headers: vec![],
                    auth: DocAuth::default(),
                    body: DocBody::default(),
                    scripts: None,
                    assertions: None,
                }],
            }],
        }
    }

    #[test]
    fn applies_meta_description_values_and_exclusion() {
        let mut data = bundle();
        let mut overlay = DocEditOverlay {
            revision: "rev1".into(),
            ..Default::default()
        };
        overlay.meta.title = Some("新标题".into());
        overlay.nodes.insert(
            "r-2".into(),
            DocNodeEdit {
                description: Some("新说明".into()),
                example_values: [("p-2-query-page-1".into(), "9".into())].into_iter().collect(),
            },
        );
        let warnings = apply(&mut data, &overlay);
        assert!(warnings.is_empty());
        assert_eq!(data.scope.title, "新标题");
        assert_eq!(data.groups[0].requests[0].description, "新说明");
        assert_eq!(data.groups[0].requests[0].query[0].value, "9");

        overlay.excluded.insert("r-2".into());
        apply(&mut data, &overlay);
        assert!(data.groups[0].requests.is_empty());
        assert_eq!(data.groups[0].children.len(), 0);
    }
}
```

```rust
// cache.rs 底部
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_expires() {
        let cache = DocsCache::new();
        let id = cache.put("x".to_string());
        assert_eq!(cache.get(&id).unwrap(), "x");
        assert!(cache.get("missing").unwrap_err().contains("E_DRAFT_EXPIRED"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::overlay docs::cache`
Expected: 编译失败

- [ ] **Step 3: 写 `overlay.rs`**

```rust
use std::collections::{HashMap, HashSet};

use super::builder::pointer_of;
use super::masking;
use super::model::*;

pub fn apply(bundle: &mut DocBundle, overlay: &DocEditOverlay) -> Vec<String> {
    let mut warnings = Vec::new();
    if !overlay.revision.is_empty() && overlay.revision != bundle.revision {
        warnings.push("源数据已变化，部分编辑可能未生效".to_string());
    }
    if let Some(title) = overlay.meta.title.as_ref().filter(|text| !text.trim().is_empty()) {
        bundle.scope.title = title.clone();
    }
    if let Some(subtitle) = &overlay.meta.subtitle {
        bundle.subtitle = subtitle.clone();
    }
    for group in &mut bundle.groups {
        apply_group(group, overlay, &mut warnings);
    }
    let excluded = &overlay.excluded;
    bundle.groups.retain(|group| !excluded.contains(&format!("g-{}", group.id)));
    for group in &mut bundle.groups {
        remove_excluded(group, excluded);
    }
    for group in &mut bundle.groups {
        reorder(group, overlay);
    }
    warnings
}

fn apply_group(group: &mut DocGroup, overlay: &DocEditOverlay, warnings: &mut Vec<String>) {
    if let Some(node) = overlay.nodes.get(&format!("g-{}", group.id)) {
        if let Some(description) = &node.description {
            group.description = description.clone();
        }
    }
    for request in &mut group.requests {
        apply_request(request, overlay, warnings);
    }
    for child in &mut group.children {
        apply_group(child, overlay, warnings);
    }
}

fn apply_request(request: &mut DocRequest, overlay: &DocEditOverlay, warnings: &mut Vec<String>) {
    let Some(node) = overlay.nodes.get(&format!("r-{}", request.id)) else {
        return;
    };
    if let Some(description) = &node.description {
        request.description = description.clone();
    }
    let mut seen = HashMap::new();
    for param in request
        .query
        .iter_mut()
        .chain(request.path_params.iter_mut())
        .chain(request.headers.iter_mut())
    {
        let occurrence = seen.entry((param.location.clone(), param.name.clone())).or_insert(0usize);
        *occurrence += 1;
        let key = format!(
            "p-{}-{}-{}-{}",
            request.id, param.location, param.name, occurrence
        );
        if masking::is_sensitive_name(&param.name) {
            continue;
        }
        if let Some(value) = node.example_values.get(&key) {
            param.value = value.clone();
            param.example_source = "manual".into();
        }
    }
    let mut body_seen = HashMap::new();
    for field in &mut request.body.fields {
        let occurrence = body_seen.entry(field.name.clone()).or_insert(0usize);
        *occurrence += 1;
        let key = if request.body.kind == "json" {
            format!("b-{}-{}", request.id, pointer_of(&field.name))
        } else {
            format!("f-{}-{}-{}", request.id, field.name, occurrence)
        };
        if masking::is_sensitive_name(&field.name) {
            continue;
        }
        if let Some(value) = node.example_values.get(&key) {
            field.value = value.clone();
            field.example_source = "manual".into();
        }
    }
    if matches!(request.body.kind.as_str(), "json" | "form" | "multipart")
        && !request.body.fields.is_empty()
    {
        rebuild_body_text(request);
    }
    if node.example_values.keys().any(|key| {
        key.starts_with(&format!("p-{}-", request.id))
            || key.starts_with(&format!("b-{}-", request.id))
            || key.starts_with(&format!("f-{}-", request.id))
    }) {
        let _ = warnings;
    }
}

fn rebuild_body_text(request: &mut DocRequest) {
    match request.body.kind.as_str() {
        "json" => {
            if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str(&request.body.text) {
                for field in &request.body.fields {
                    let value = serde_json::from_str::<serde_json::Value>(&field.value)
                        .unwrap_or_else(|_| serde_json::Value::String(field.value.clone()));
                    map.insert(field.name.clone(), value);
                }
                if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Object(map)) {
                    request.body.text = text;
                }
            }
        }
        "form" => {
            request.body.text = request
                .body
                .fields
                .iter()
                .map(|field| format!("{}={}", field.name, field.value))
                .collect::<Vec<_>>()
                .join("&");
        }
        "multipart" => {
            let fields: Vec<serde_json::Value> = request
                .body
                .fields
                .iter()
                .map(|field| {
                    serde_json::json!({
                        "key": field.name,
                        "value": field.value,
                        "enabled": true,
                        "kind": "text"
                    })
                })
                .collect();
            if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Array(fields)) {
                request.body.text = text;
            }
        }
        _ => {}
    }
}

fn remove_excluded(group: &mut DocGroup, excluded: &HashSet<String>) {
    group
        .requests
        .retain(|request| !excluded.contains(&format!("r-{}", request.id)));
    group
        .children
        .retain(|child| !excluded.contains(&format!("g-{}", child.id)));
    for child in &mut group.children {
        remove_excluded(child, excluded);
    }
}

fn reorder(group: &mut DocGroup, overlay: &DocEditOverlay) {
    if let Some(order) = overlay.order.get(&format!("g-{}", group.id)) {
        let rank: HashMap<&str, usize> =
            order.iter().enumerate().map(|(index, key)| (key.as_str(), index)).collect();
        group.requests.sort_by_key(|request| {
            rank.get(format!("r-{}", request.id).as_str()).copied().unwrap_or(usize::MAX)
        });
    }
    for child in &mut group.children {
        reorder(child, overlay);
    }
}
```

- [ ] **Step 4: 写 `cache.rs`**

```rust
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::model::DocBundle;

const TTL: Duration = Duration::from_secs(30 * 60);
const MAX_DRAFTS: usize = 3;

struct Cached {
    bundle: DocBundle,
    created: Instant,
}

#[derive(Default)]
struct Inner {
    drafts: HashMap<String, Cached>,
    order: VecDeque<String>,
}

impl Inner {
    fn purge(&mut self) {
        let now = Instant::now();
        let expired: Vec<String> = self
            .drafts
            .iter()
            .filter(|(_, item)| now.duration_since(item.created) > TTL)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.drafts.remove(&id);
        }
        self.order.retain(|id| self.drafts.contains_key(id));
    }
}

#[derive(Default)]
pub struct DocsCache {
    inner: Mutex<Inner>,
}

impl DocsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&self, bundle: DocBundle) -> String {
        let mut inner = self.inner.lock();
        inner.purge();
        let id = uuid::Uuid::new_v4().to_string();
        inner.drafts.insert(
            id.clone(),
            Cached {
                bundle,
                created: Instant::now(),
            },
        );
        inner.order.push_back(id.clone());
        while inner.order.len() > MAX_DRAFTS {
            if let Some(old) = inner.order.pop_front() {
                inner.drafts.remove(&old);
            }
        }
        id
    }

    pub fn get(&self, id: &str) -> Result<DocBundle, String> {
        let mut inner = self.inner.lock();
        inner.purge();
        inner
            .drafts
            .get(id)
            .map(|item| item.bundle.clone())
            .ok_or_else(|| "E_DRAFT_EXPIRED: 草稿已过期，请重新生成".to_string())
    }
}
```

- [ ] **Step 5: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs::overlay docs::cache`
Expected: PASS

---

### Task 6: Tauri 命令、AppState 与文件落盘

**Files:**
- Create: `src-tauri/src/commands/docs.rs`
- Modify: `src-tauri/src/commands/mod.rs`（加 `pub mod docs;`）
- Modify: `src-tauri/src/state.rs`（两个构造函数各加 `docs_cache`）
- Modify: `src-tauri/src/lib.rs`（注册三个命令）

**Interfaces:**
- Produces: `docs_prepare` / `docs_render` / `docs_export`（payload 结构与 5.3 一致）

- [ ] **Step 1: 写测试（命令文件底部，只测扩展名与大小检查）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mismatched_extension() {
        assert!(check_extension("/tmp/a.md", "markdown").is_ok());
        assert!(check_extension("/tmp/a.html", "html").is_ok());
        assert!(check_extension("/tmp/a.html", "markdown").unwrap_err().contains("E_FORMAT"));
        assert!(check_extension("/tmp/a.pdf", "pdf").unwrap_err().contains("E_FORMAT"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::docs`
Expected: 编译失败

- [ ] **Step 3: 写 `commands/docs.rs`**

```rust
use tauri::State;

use crate::docs::{builder, cache::DocsCache, model::*, overlay, render_html, render_markdown};
use crate::state::AppState;
use crate::util::err;

fn render_bundle(bundle: &DocBundle, format: &str) -> Result<String, String> {
    match format {
        "markdown" => Ok(render_markdown::render(bundle)),
        "html" => Ok(render_html::render(bundle)),
        other => Err(format!("E_FORMAT: 不支持的格式 {other}")),
    }
}

fn check_extension(path: &str, format: &str) -> Result<(), String> {
    let lower = path.to_ascii_lowercase();
    let matched = match format {
        "markdown" => lower.ends_with(".md"),
        "html" => lower.ends_with(".html") || lower.ends_with(".htm"),
        other => return Err(format!("E_FORMAT: 不支持的格式 {other}")),
    };
    if matched {
        Ok(())
    } else {
        Err(format!("E_FORMAT: 扩展名与格式不匹配（{format}）"))
    }
}

fn write_text(path: &str, content: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("导出路径为空".to_string());
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建目录失败（{}）：{error}", parent.display()))?;
        }
    }
    std::fs::write(path, content).map_err(|error| format!("写入文件失败（{path}）：{error}"))
}

#[tauri::command(async)]
pub fn docs_prepare(
    state: State<'_, AppState>,
    payload: DocsPreparePayload,
) -> Result<DocsPrepareResult, String> {
    let conn = state.pool.get().map_err(err)?;
    let bundle = builder::build(&conn, &payload)?;
    let outline = builder::outline(&bundle);
    let size = serde_json::to_vec(&outline).map_err(err)?.len();
    if size > 5 * 1024 * 1024 {
        return Err("E_TOO_LARGE: 大纲超过 5 MB，请按集合分批导出".to_string());
    }
    let result = DocsPrepareResult {
        draft_id: String::new(),
        revision: bundle.revision.clone(),
        meta: DocMetaDefaults {
            title: bundle.scope.title.clone(),
            subtitle: bundle.subtitle.clone(),
        },
        outline,
        warnings: bundle.warnings.clone(),
    };
    let draft_id = state.docs_cache.put(bundle);
    Ok(DocsPrepareResult {
        draft_id,
        ..result
    })
}

#[tauri::command(async)]
pub fn docs_render(
    state: State<'_, AppState>,
    payload: DocsRenderPayload,
) -> Result<DocsRenderResult, String> {
    let mut bundle = state.docs_cache.get(&payload.draft_id)?;
    let warnings = overlay::apply(&mut bundle, &payload.overlay);
    let content = render_bundle(&bundle, &payload.format)?;
    if content.len() > 20 * 1024 * 1024 {
        return Err("E_TOO_LARGE: 预览内容超过 20 MB，请改用 Markdown 或分批导出".to_string());
    }
    Ok(DocsRenderResult {
        bytes: content.len() as u64,
        content,
        warnings,
    })
}

#[tauri::command(async)]
pub fn docs_export(
    state: State<'_, AppState>,
    payload: DocsExportPayload,
) -> Result<DocsExportResult, String> {
    check_extension(&payload.path, &payload.format)?;
    let (mut bundle, mut warnings) = match payload.source {
        DocsExportSource::Scope(prepare) => {
            let conn = state.pool.get().map_err(err)?;
            (builder::build(&conn, &prepare)?, Vec::new())
        }
        DocsExportSource::Draft { draft_id, overlay } => {
            let mut bundle = state.docs_cache.get(&draft_id)?;
            let warnings = overlay::apply(&mut bundle, &overlay);
            (bundle, warnings)
        }
    };
    let content = render_bundle(&bundle, &payload.format)?;
    if content.len() > 20 * 1024 * 1024 {
        return Err("E_TOO_LARGE: 导出内容超过 20 MB，请按集合分批导出".to_string());
    }
    write_text(&payload.path, &content)?;
    warnings.extend(bundle.warnings.clone());
    Ok(DocsExportResult {
        path: payload.path,
        format: payload.format,
        scope: bundle.scope.kind,
        request_count: builder::request_count(&bundle),
        collection_count: builder::collection_count(&bundle),
        bytes: content.len() as u64,
        warnings,
        pdf_engine: None,
        fallback_html_path: None,
    })
}
```

> 删除未使用的 `DocsCache` 导入（`cache::DocsCache`），`state.docs_cache` 已经类型确定。

- [ ] **Step 4: 接入 AppState**

`src-tauri/src/state.rs` 在 `AppState` 字段末尾加：

```rust
    /// 接口文档草稿缓存：draft_id → IR（TTL / LRU）
    pub docs_cache: crate::docs::cache::DocsCache,
```

两个构造函数（`new` 与 `headless_at`）的 `Ok(Self { ... })` 里都加：

```rust
            docs_cache: crate::docs::cache::DocsCache::new(),
```

- [ ] **Step 5: 注册命令**

`src-tauri/src/commands/mod.rs` 加：

```rust
pub mod docs;
```

`src-tauri/src/lib.rs` 的 `invoke_handler` 中，在 `commands::environment::env_script_save,` 后加：

```rust
            commands::docs::docs_prepare,
            commands::docs::docs_render,
            commands::docs::docs_export,
```

- [ ] **Step 6: 编译并跑测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::docs docs:: -- --nocapture`
Expected: 全部 PASS；若提示 `docs_cache` 未初始化，检查 `state.rs` 两个构造函数。

---

### Task 7: 前端类型、API 与 `docsExport` 工具

**Files:**
- Create: `src/types/docs.ts`
- Create: `src/utils/docsExport.ts`
- Create: `src/utils/__tests__/docsExport.spec.ts`
- Modify: `src/api/index.ts`（导入类型 + 三个方法）

**Interfaces:**
- Produces:
  - `@/types/docs` 中的全部类型
  - `api.docsPrepare / api.docsRender / api.docsExport`
  - `docsExport.ts` 的 `sanitizeFileName / extensionFor / overlayKey / loadOverlay / saveOverlay / clearOverlay / countEdits / debounce / emptyOverlay`

- [ ] **Step 1: 写前端失败测试**

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  countEdits,
  emptyOverlay,
  extensionFor,
  loadOverlay,
  overlayKey,
  sanitizeFileName,
  saveOverlay,
} from '@/utils/docsExport'

function memoryStorage() {
  const map = new Map<string, string>()
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
    clear: () => map.clear(),
    key: (index: number) => [...map.keys()][index] ?? null,
    get length() {
      return map.size
    },
  } as Storage
}

beforeEach(() => {
  Object.defineProperty(globalThis, 'localStorage', {
    value: memoryStorage(),
    configurable: true,
  })
})

describe('docsExport', () => {
  it('净化文件名并保留中文', () => {
    expect(sanitizeFileName('开放平台 / 登录: v2?')).toBe('开放平台 _ 登录_ v2_')
    expect(sanitizeFileName('   ')).toBe('接口文档')
    expect(sanitizeFileName('a'.repeat(200)).length).toBe(120)
  })

  it('格式扩展名映射', () => {
    expect(extensionFor('markdown')).toBe('.md')
    expect(extensionFor('html')).toBe('.html')
  })

  it('覆盖层按 scope + targetId + environmentId 隔离', () => {
    const overlay = emptyOverlay('rev1')
    overlay.meta.title = '标题'
    saveOverlay('project', 1, 2, overlay)
    expect(loadOverlay('project', 1, 2)?.meta.title).toBe('标题')
    expect(loadOverlay('project', 1, 3)).toBeNull()
    expect(overlayKey('project', 1, 2)).toBe('astroman:docs-overlay:project:1:2')
  })

  it('统计编辑数量', () => {
    const overlay = emptyOverlay('rev1')
    expect(countEdits(overlay)).toBe(0)
    overlay.meta.title = '标题'
    overlay.nodes['r-1'] = { description: '说明', exampleValues: { 'p-1-query-a-1': 'x' } }
    overlay.excluded = ['r-2']
    expect(countEdits(overlay)).toBe(4)
  })

  it('超过 1 MB 不写 localStorage 但不清空内存对象', () => {
    const overlay = emptyOverlay('rev1')
    overlay.nodes['r-1'] = {
      description: 'x',
      exampleValues: { k: 'y'.repeat(1024 * 1024 + 1) },
    }
    saveOverlay('project', 1, 2, overlay)
    expect(loadOverlay('project', 1, 2)).toBeNull()
    expect(overlay.nodes['r-1'].exampleValues.k.length).toBeGreaterThan(1024 * 1024)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `npm test -- docsExport`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 写 `src/types/docs.ts`**

```ts
export type DocScope = 'request' | 'collection' | 'project'
export type DocFormat = 'markdown' | 'html'

export interface DocsOptions {
  includeEnvironment: boolean
  includeScripts: boolean
  includeAssertions: boolean
  includeResponseExample: boolean
  resolveEnvironment: boolean
  useRunExamples: boolean
  autoExamples: boolean
  runExamplesMaxAgeDays: number
  pageBreakPerRequest: boolean
  includeGeneratedAt: boolean
}

export const defaultDocsOptions = (): DocsOptions => ({
  includeEnvironment: true,
  includeScripts: false,
  includeAssertions: false,
  includeResponseExample: false,
  resolveEnvironment: true,
  useRunExamples: true,
  autoExamples: true,
  runExamplesMaxAgeDays: 30,
  pageBreakPerRequest: true,
  includeGeneratedAt: false,
})

export interface DocOutlineField {
  key: string
  name: string
  location: string
  value: string
  source: string
  sensitive: boolean
}

export interface DocOutlineRequest {
  key: string
  name: string
  method: string
  url: string
  description: string
  included: boolean
  params: DocOutlineField[]
  warnings: string[]
}

export interface DocOutlineGroup {
  key: string
  name: string
  path: string[]
  requests: DocOutlineRequest[]
  children: DocOutlineGroup[]
}

export interface DocOutline {
  groups: DocOutlineGroup[]
}

export interface DocNodeEdit {
  description?: string
  exampleValues: Record<string, string>
}

export interface DocEditOverlay {
  revision: string
  meta: { title?: string; subtitle?: string }
  nodes: Record<string, DocNodeEdit>
  excluded: string[]
  order: Record<string, string[]>
}

export interface DocMetaDefaults {
  title: string
  subtitle: string
}

export interface DocsPreparePayload {
  scope: DocScope
  targetId: number
  environmentId: number | null
  options: DocsOptions
}

export interface DocsPrepareResult {
  draftId: string
  revision: string
  meta: DocMetaDefaults
  outline: DocOutline
  warnings: string[]
}

export interface DocsRenderPayload {
  draftId: string
  overlay: DocEditOverlay
  format: DocFormat
}

export interface DocsRenderResult {
  content: string
  bytes: number
  warnings: string[]
}

export type DocsExportSource =
  | { kind: 'scope'; scope: DocScope; targetId: number; environmentId: number | null; options: DocsOptions }
  | { kind: 'draft'; draftId: string; overlay: DocEditOverlay }

export interface DocsExportPayload {
  format: DocFormat
  path: string
  source: DocsExportSource
}

export interface DocsExportResult {
  path: string
  format: string
  scope: string
  requestCount: number
  collectionCount: number
  bytes: number
  warnings: string[]
  pdfEngine: string | null
  fallbackHtmlPath: string | null
}
```

- [ ] **Step 4: 写 `src/utils/docsExport.ts`**

```ts
import type { DocEditOverlay, DocFormat, DocScope } from '@/types/docs'

const MAX_OVERLAY_BYTES = 1024 * 1024
const STORAGE_PREFIX = 'astroman:docs-overlay'

export function sanitizeFileName(input: string, fallback = '接口文档'): string {
  const cleaned = input
    .replace(/[\\/:*?"<>|]/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
  return (cleaned || fallback).slice(0, 120)
}

export function extensionFor(format: DocFormat): string {
  return format === 'markdown' ? '.md' : '.html'
}

export function overlayKey(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
): string {
  return `${STORAGE_PREFIX}:${scope}:${targetId}:${environmentId ?? 'default'}`
}

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage
  } catch {
    return null
  }
}

export function emptyOverlay(revision = ''): DocEditOverlay {
  return { revision, meta: {}, nodes: {}, excluded: [], order: {} }
}

export function loadOverlay(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
): DocEditOverlay | null {
  const store = storage()
  if (!store) return null
  try {
    const raw = store.getItem(overlayKey(scope, targetId, environmentId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<DocEditOverlay>
    return {
      revision: parsed.revision ?? '',
      meta: parsed.meta ?? {},
      nodes: parsed.nodes ?? {},
      excluded: parsed.excluded ?? [],
      order: parsed.order ?? {},
    }
  } catch {
    return null
  }
}

/** 超过 1 MB 时只跳过 localStorage 写入，内存中的覆盖层仍可导出 */
export function saveOverlay(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
  overlay: DocEditOverlay,
): void {
  const store = storage()
  if (!store) return
  const raw = JSON.stringify(overlay)
  if (new Blob([raw]).size > MAX_OVERLAY_BYTES) return
  try {
    store.setItem(overlayKey(scope, targetId, environmentId), raw)
  } catch {
    // 配额满 / 隐私模式：退化为内存暂存，不阻断导出
  }
}

export function clearOverlay(
  scope: DocScope,
  targetId: number,
  environmentId: number | null,
): void {
  const store = storage()
  try {
    store?.removeItem(overlayKey(scope, targetId, environmentId))
  } catch {
    // 忽略
  }
}

export function countEdits(overlay: DocEditOverlay): number {
  let count = 0
  if (overlay.meta.title !== undefined) count += 1
  if (overlay.meta.subtitle !== undefined) count += 1
  for (const node of Object.values(overlay.nodes)) {
    if (node.description !== undefined) count += 1
    count += Object.keys(node.exampleValues ?? {}).length
  }
  count += overlay.excluded.length
  count += Object.values(overlay.order).filter((keys) => keys.length > 0).length
  return count
}

export function debounce<T extends (...args: never[]) => void>(fn: T, wait = 300): T {
  let timer: ReturnType<typeof setTimeout> | null = null
  return ((...args: never[]) => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => fn(...args), wait)
  }) as T
}
```

- [ ] **Step 5: 在 `src/api/index.ts` 增加类型与方法**

在类型导入块加：

```ts
import type {
  DocsExportPayload,
  DocsExportResult,
  DocsPreparePayload,
  DocsPrepareResult,
  DocsRenderPayload,
  DocsRenderResult,
} from '@/types/docs'
```

在 `openapiExport` 后加：

```ts
// 接口文档导出（阶段一：Markdown / HTML）
export const docsPrepare = (payload: DocsPreparePayload) =>
  invoke<DocsPrepareResult>('docs_prepare', { payload })

export const docsRender = (payload: DocsRenderPayload) =>
  invoke<DocsRenderResult>('docs_render', { payload })

export const docsExport = (payload: DocsExportPayload) =>
  invoke<DocsExportResult>('docs_export', { payload })
```

- [ ] **Step 6: 运行前端测试**

Run: `npm test -- docsExport`
Expected: 全部 PASS

Run: `npx vue-tsc --noEmit`
Expected: 无新增错误（若 `Blob` 类型报错，改用 `new TextEncoder().encode(raw).length`）。

---

### Task 8: 预览抽屉与编辑树

**Files:**
- Create: `src/components/DocsOutlineNode.vue`
- Create: `src/components/DocsPreviewDrawer.vue`

**Interfaces:**
- Consumes: `api.docsPrepare / docsRender / docsExport`、`@/types/docs`、`@/utils/docsExport`、`envList`
- Produces:
  - `DocsPreviewDrawer` props：`open: boolean`、`scope: DocScope`、`targetId: number`、`projectId: number`、`projectName?: string`、`defaultEnvironmentId?: number | null`
  - emits：`update:open`

- [ ] **Step 1: 写 `DocsOutlineNode.vue`**

```vue
<script setup lang="ts">
import { computed, ref } from 'vue'
import type { DocEditOverlay, DocOutlineGroup, DocOutlineRequest } from '@/types/docs'

const props = defineProps<{
  group: DocOutlineGroup
  overlay: DocEditOverlay
  depth: number
}>()

const emit = defineEmits<{
  (e: 'toggle-request', key: string): void
  (e: 'toggle-group', key: string): void
  (e: 'description', key: string, value: string): void
  (e: 'field', requestKey: string, fieldKey: string, value: string): void
  (e: 'move-request', groupKey: string, from: number, to: number): void
}>()

const dragFrom = ref(-1)
const groupExcluded = computed(() => props.overlay.excluded.includes(props.group.key))

const orderedRequests = computed(() => {
  const order = props.overlay.order[props.group.key]
  if (!order?.length) return props.group.requests
  const rank = new Map(order.map((key, index) => [key, index]))
  return [...props.group.requests].sort(
    (left, right) => (rank.get(left.key) ?? 9999) - (rank.get(right.key) ?? 9999),
  )
})

function isExcluded(key: string): boolean {
  return props.overlay.excluded.includes(key)
}

function descriptionOf(request: DocOutlineRequest): string {
  return props.overlay.nodes[request.key]?.description ?? request.description
}

function fieldValue(request: DocOutlineRequest, key: string, fallback: string): string {
  return props.overlay.nodes[request.key]?.exampleValues?.[key] ?? fallback
}

function onDrop(index: number) {
  if (dragFrom.value >= 0 && dragFrom.value !== index) {
    emit('move-request', props.group.key, dragFrom.value, index)
  }
  dragFrom.value = -1
}
</script>

<template>
  <div class="outline-group" :style="{ paddingLeft: `${depth * 14}px` }">
    <div class="group-row">
      <a-checkbox
        :checked="!groupExcluded"
        @change="emit('toggle-group', group.key)"
      />
      <span class="group-name">{{ group.name }}</span>
      <span class="group-path">{{ group.path.join(' / ') }}</span>
    </div>

    <div
      v-for="(request, index) in orderedRequests"
      :key="request.key"
      class="request-row"
      draggable="true"
      @dragstart="dragFrom = index"
      @dragover.prevent
      @drop="onDrop(index)"
    >
      <a-checkbox
        :checked="!isExcluded(request.key)"
        @change="emit('toggle-request', request.key)"
      />
      <span class="method">{{ request.method }}</span>
      <span class="request-name" :title="request.url">{{ request.name }}</span>
      <a-popover trigger="click" placement="right" :overlay-style="{ width: '360px' }">
        <template #content>
          <div class="edit-panel">
            <a-textarea
              :value="descriptionOf(request)"
              :rows="3"
              placeholder="接口说明"
              @change="(event: any) => emit('description', request.key, event.target.value)"
            />
            <div v-for="field in request.params" :key="field.key" class="edit-field">
              <span class="edit-label">{{ field.location }} · {{ field.name }}</span>
              <a-input
                size="small"
                :value="fieldValue(request, field.key, field.value)"
                :disabled="field.sensitive"
                @change="(event: any) => emit('field', request.key, field.key, event.target.value)"
              />
            </div>
          </div>
        </template>
        <a-button size="small" type="text">编辑</a-button>
      </a-popover>
    </div>

    <DocsOutlineNode
      v-for="child in group.children"
      :key="child.key"
      :group="child"
      :overlay="overlay"
      :depth="depth + 1"
      @toggle-request="(key: string) => emit('toggle-request', key)"
      @toggle-group="(key: string) => emit('toggle-group', key)"
      @description="(key: string, value: string) => emit('description', key, value)"
      @field="(requestKey: string, fieldKey: string, value: string) => emit('field', requestKey, fieldKey, value)"
      @move-request="(groupKey: string, from: number, to: number) => emit('move-request', groupKey, from, to)"
    />
  </div>
</template>

<style scoped>
.outline-group {
  border-left: 1px solid #f0f0f0;
  margin-left: 2px;
}
.group-row,
.request-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 30px;
  padding: 2px 4px;
}
.group-row {
  font-weight: 600;
}
.group-name {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.group-path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #8c8c8c;
  font-size: 12px;
  font-weight: 400;
}
.request-row {
  cursor: move;
}
.request-row:hover {
  background: #f5f6f7;
}
.method {
  min-width: 42px;
  color: #009688;
  font-size: 12px;
  font-weight: 600;
}
.request-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.edit-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 420px;
  overflow: auto;
}
.edit-field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.edit-label {
  color: #8c8c8c;
  font-size: 12px;
}
</style>
```

- [ ] **Step 2: 写 `DocsPreviewDrawer.vue`**

```vue
<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { message, Modal } from 'ant-design-vue'
import { save } from '@tauri-apps/plugin-dialog'
import MarkdownIt from 'markdown-it'
import * as api from '@/api'
import type { Environment } from '@/types/models'
import type {
  DocEditOverlay,
  DocFormat,
  DocOutline,
  DocOutlineGroup,
  DocScope,
  DocsOptions,
  DocsPrepareResult,
} from '@/types/docs'
import { defaultDocsOptions } from '@/types/docs'
import {
  clearOverlay,
  countEdits,
  debounce,
  emptyOverlay,
  extensionFor,
  loadOverlay,
  sanitizeFileName,
  saveOverlay,
} from '@/utils/docsExport'
import DocsOutlineNode from './DocsOutlineNode.vue'

const props = defineProps<{
  open: boolean
  scope: DocScope
  targetId: number
  projectId: number
  projectName?: string
  defaultEnvironmentId?: number | null
}>()

const emit = defineEmits<{ (e: 'update:open', value: boolean): void }>()

const md = new MarkdownIt({ html: false, linkify: true, breaks: false })

const loading = ref(false)
const exporting = ref(false)
const environments = ref<Environment[]>([])
const environmentId = ref<number | null>(props.defaultEnvironmentId ?? null)
const options = reactive<DocsOptions>(defaultDocsOptions())
const outline = ref<DocOutline | null>(null)
const overlay = ref<DocEditOverlay>(emptyOverlay())
const draftId = ref('')
const revision = ref('')
const warnings = ref<string[]>([])
const title = ref('')
const subtitle = ref('')
const tab = ref<'html' | 'markdown' | 'print'>('html')
const html = ref('')
const markdown = ref('')

const edited = computed(() => countEdits(overlay.value))
const markdownHtml = computed(() => md.render(markdown.value))

async function loadEnvironments() {
  if (!props.projectId) return
  try {
    const list = await api.envList(props.projectId)
    environments.value = list
    if (environmentId.value == null) {
      environmentId.value = list.find((item) => item.isActive)?.id ?? list[0]?.id ?? null
    }
  } catch (error) {
    message.error(String(error))
  }
}

function persist() {
  saveOverlay(props.scope, props.targetId, environmentId.value, overlay.value)
}

async function prepare(keepOverlay = true) {
  loading.value = true
  try {
    const result: DocsPrepareResult = await api.docsPrepare({
      scope: props.scope,
      targetId: props.targetId,
      environmentId: environmentId.value,
      options: { ...options },
    })
    draftId.value = result.draftId
    revision.value = result.revision
    outline.value = result.outline
    warnings.value = result.warnings
    if (!overlay.value.meta.title) title.value = result.meta.title
    if (overlay.value.meta.subtitle === undefined) subtitle.value = result.meta.subtitle
    overlay.value.revision = result.revision
    if (!keepOverlay) {
      overlay.value = emptyOverlay(result.revision)
      title.value = result.meta.title
      subtitle.value = result.meta.subtitle
    }
    persist()
    await renderNow()
  } catch (error) {
    message.error(String(error))
  } finally {
    loading.value = false
  }
}

async function renderNow() {
  if (!draftId.value) return
  try {
    const result = await api.docsRender({
      draftId: draftId.value,
      overlay: overlay.value,
      format: tab.value === 'markdown' ? 'markdown' : 'html',
    })
    if (tab.value === 'markdown') markdown.value = result.content
    else html.value = result.content
    if (tab.value === 'html' && !markdown.value) {
      const source = await api.docsRender({
        draftId: draftId.value,
        overlay: overlay.value,
        format: 'markdown',
      })
      markdown.value = source.content
    }
  } catch (error) {
    message.error(String(error))
  }
}

const renderSoon = debounce(() => void renderNow(), 300)

function touch() {
  persist()
  renderSoon()
}

function setTitle(value: string) {
  title.value = value
  overlay.value.meta.title = value
  touch()
}

function setSubtitle(value: string) {
  subtitle.value = value
  overlay.value.meta.subtitle = value
  touch()
}

function toggleExcluded(key: string) {
  const set = new Set(overlay.value.excluded)
  if (set.has(key)) set.delete(key)
  else set.add(key)
  overlay.value.excluded = [...set]
  touch()
}

function setDescription(key: string, value: string) {
  const node = overlay.value.nodes[key] ?? { exampleValues: {} }
  overlay.value.nodes[key] = { ...node, description: value }
  touch()
}

function setField(requestKey: string, fieldKey: string, value: string) {
  const node = overlay.value.nodes[requestKey] ?? { exampleValues: {} }
  node.exampleValues = { ...(node.exampleValues ?? {}), [fieldKey]: value }
  overlay.value.nodes[requestKey] = node
  touch()
}

function findGroup(list: DocOutlineGroup[], key: string): DocOutlineGroup | null {
  for (const group of list) {
    if (group.key === key) return group
    const child = findGroup(group.children, key)
    if (child) return child
  }
  return null
}

function moveRequest(groupKey: string, from: number, to: number) {
  const group = outline.value ? findGroup(outline.value.groups, groupKey) : null
  if (!group) return
  const order = overlay.value.order[groupKey] ?? group.requests.map((item) => item.key)
  const [moved] = order.splice(from, 1)
  if (!moved) return
  order.splice(to, 0, moved)
  overlay.value.order = { ...overlay.value.order, [groupKey]: order }
  touch()
}

function confirmDiscard(text: string): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: '确认放弃编辑？',
      content: text,
      okText: '继续',
      cancelText: '取消',
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    })
  })
}

async function onEnvironmentChange(value: number | null) {
  if (edited.value > 0 && !(await confirmDiscard('切换环境会重新生成示例值，已编辑内容将丢失'))) {
    return
  }
  environmentId.value = value
  clearOverlay(props.scope, props.targetId, value)
  overlay.value = emptyOverlay()
  title.value = ''
  subtitle.value = ''
  await prepare(false)
}

async function onOptionsChange() {
  await prepare(true)
}

async function regenerate() {
  if (edited.value > 0 && !(await confirmDiscard('将丢弃手动编辑并重新生成示例值'))) return
  clearOverlay(props.scope, props.targetId, environmentId.value)
  overlay.value = emptyOverlay()
  await prepare(false)
}

function open() {
  emit('update:open', false)
}

async function exportDoc(format: DocFormat) {
  if (!draftId.value) return
  const path = await save({
    title: '导出接口文档',
    defaultPath: `${sanitizeFileName(title.value || props.projectName || '接口文档')}${extensionFor(format)}`,
    filters: [
      {
        name: format === 'markdown' ? 'Markdown' : 'HTML',
        extensions: [format === 'markdown' ? 'md' : 'html'],
      },
    ],
  })
  if (!path) return
  exporting.value = true
  try {
    const call = async () =>
      api.docsExport({
        format,
        path,
        source: { kind: 'draft', draftId: draftId.value, overlay: overlay.value },
      })
    let result
    try {
      result = await call()
    } catch (error) {
      if (!String(error).includes('E_DRAFT_EXPIRED')) throw error
      await prepare(true)
      result = await call()
    }
    message.success(`已导出 ${result.requestCount} 个接口到 ${path}`)
    const list = result.warnings.slice(0, 3)
    if (list.length) {
      message.warning(`${list.join('；')}${result.warnings.length > 3 ? ` 等 ${result.warnings.length} 条` : ''}`)
    }
  } catch (error) {
    message.error(String(error))
  } finally {
    exporting.value = false
  }
}

function onExportMenu({ key }: { key: string | number }) {
  void exportDoc(String(key) as DocFormat)
}

watch(
  () => props.open,
  async (value) => {
    if (!value) return
    await loadEnvironments()
    const restored = loadOverlay(props.scope, props.targetId, environmentId.value)
    overlay.value = restored ?? emptyOverlay()
    title.value = restored?.meta.title ?? ''
    subtitle.value = restored?.meta.subtitle ?? ''
    await prepare(true)
  },
  { immediate: true },
)

watch(tab, () => void renderNow())
</script>

<template>
  <a-drawer
    :open="open"
    placement="right"
    width="100vw"
    :closable="false"
    :body-style="{ padding: '0', height: '100vh' }"
    @close="open"
  >
    <div class="docs-root">
      <header class="docs-head">
        <div class="head-title">
          <span class="head-name">导出接口文档</span>
          <span class="head-scope">{{ scope }} · {{ projectName || '' }}</span>
          <a-tag v-if="edited" color="orange">已编辑 {{ edited }} 处</a-tag>
        </div>
        <div class="head-actions">
          <a-button size="small" :loading="loading" @click="regenerate">重新生成</a-button>
          <a-dropdown :trigger="['click']">
            <a-button type="primary" size="small" :loading="exporting">
              导出 <span class="caret">▾</span>
            </a-button>
            <template #overlay>
              <a-menu @click="onExportMenu">
                <a-menu-item key="markdown">Markdown（.md）</a-menu-item>
                <a-menu-item key="html">独立 HTML（.html）</a-menu-item>
              </a-menu>
            </template>
          </a-dropdown>
          <a-button size="small" type="text" @click="open">关闭</a-button>
        </div>
      </header>

      <div class="docs-body">
        <aside class="docs-side">
          <div class="side-block">
            <div class="block-title">环境</div>
            <a-select
              :value="environmentId"
              style="width: 100%"
              placeholder="不解析环境变量"
              allow-clear
              @change="onEnvironmentChange"
            >
              <a-select-option v-for="env in environments" :key="env.id" :value="env.id">
                {{ env.name }}
              </a-select-option>
            </a-select>
          </div>

          <div class="side-block">
            <div class="block-title">设置</div>
            <a-checkbox v-model:checked="options.includeEnvironment" @change="onOptionsChange">
              环境变量表
            </a-checkbox>
            <a-checkbox v-model:checked="options.resolveEnvironment" @change="onOptionsChange">
              变量转真实值
            </a-checkbox>
            <a-checkbox v-model:checked="options.includeScripts" @change="onOptionsChange">
              脚本
            </a-checkbox>
            <a-checkbox v-model:checked="options.includeAssertions" @change="onOptionsChange">
              断言
            </a-checkbox>
            <a-checkbox v-model:checked="options.useRunExamples" @change="onOptionsChange">
              取上次运行示例
            </a-checkbox>
            <a-checkbox v-model:checked="options.autoExamples" @change="onOptionsChange">
              自动补示例
            </a-checkbox>
          </div>

          <div class="side-block">
            <div class="block-title">文档标题</div>
            <a-input :value="title" placeholder="文档标题" @change="(event: any) => setTitle(event.target.value)" />
            <a-input
              :value="subtitle"
              placeholder="副标题（可选）"
              style="margin-top: 6px"
              @change="(event: any) => setSubtitle(event.target.value)"
            />
          </div>

          <div class="side-block outline-block">
            <div class="block-title">集合树</div>
            <div v-if="outline?.groups?.length" class="outline-tree">
              <DocsOutlineNode
                v-for="group in outline.groups"
                :key="group.key"
                :group="group"
                :overlay="overlay"
                :depth="0"
                @toggle-request="toggleExcluded"
                @toggle-group="toggleExcluded"
                @description="setDescription"
                @field="setField"
                @move-request="moveRequest"
              />
            </div>
            <a-empty v-else description="暂无接口" :image="null" />
          </div>
        </aside>

        <section class="docs-preview">
          <div class="preview-tabs">
            <a-radio-group v-model:value="tab" size="small" button-style="solid">
              <a-radio-button value="html">渲染效果</a-radio-button>
              <a-radio-button value="markdown">Markdown</a-radio-button>
              <a-radio-button value="print">打印分页</a-radio-button>
            </a-radio-group>
            <span class="preview-tip">
              {{ tab === 'print' ? 'PDF 预览为排版近似，最终以导出文件为准' : '' }}
            </span>
          </div>
          <div class="preview-body">
            <iframe
              v-if="tab !== 'markdown'"
              class="preview-frame"
              :class="{ print: tab === 'print' }"
              sandbox=""
              :srcdoc="html"
            />
            <div v-else class="markdown-view" v-html="markdownHtml" />
          </div>
          <div v-if="warnings.length" class="preview-warnings">
            <a-tag v-for="warning in warnings.slice(0, 3)" :key="warning" color="orange">
              {{ warning }}
            </a-tag>
            <span v-if="warnings.length > 3">等 {{ warnings.length }} 条</span>
          </div>
        </section>
      </div>
    </div>
  </a-drawer>
</template>

<style scoped>
.docs-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}
.docs-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid #e5e6eb;
}
.head-title {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.head-name {
  font-size: 15px;
  font-weight: 600;
}
.head-scope {
  color: #8c8c8c;
  font-size: 12px;
}
.head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.caret {
  margin-left: 4px;
}
.docs-body {
  display: flex;
  flex: 1;
  min-height: 0;
}
.docs-side {
  width: 340px;
  flex: none;
  border-right: 1px solid #e5e6eb;
  overflow: auto;
  padding: 8px;
  background: #fafafa;
}
.side-block {
  margin-bottom: 12px;
  padding: 8px;
  border-radius: 6px;
  background: #fff;
}
.side-block :deep(.ant-checkbox-wrapper) {
  display: flex;
  margin: 2px 0;
}
.block-title {
  margin-bottom: 6px;
  color: #646a73;
  font-size: 12px;
  font-weight: 600;
}
.outline-block {
  min-height: 200px;
}
.outline-tree {
  max-height: calc(100vh - 420px);
  overflow: auto;
}
.docs-preview {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  background: #f0f2f5;
}
.preview-tabs {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 12px;
  border-bottom: 1px solid #e5e6eb;
  background: #fff;
}
.preview-tip {
  color: #8c8c8c;
  font-size: 12px;
}
.preview-body {
  flex: 1;
  min-height: 0;
  padding: 12px;
}
.preview-frame {
  width: 100%;
  height: 100%;
  border: 1px solid #e5e6eb;
  border-radius: 6px;
  background: #fff;
}
.preview-frame.print {
  width: 794px;
  max-width: 100%;
  height: 100%;
  margin: 0 auto;
  display: block;
}
.markdown-view {
  height: 100%;
  overflow: auto;
  padding: 20px 24px;
  border: 1px solid #e5e6eb;
  border-radius: 6px;
  background: #fff;
  font-size: 14px;
  line-height: 1.7;
}
.preview-warnings {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  background: #fff;
  border-top: 1px solid #e5e6eb;
  font-size: 12px;
}
</style>
```

- [ ] **Step 3: 前端类型检查**

Run: `npx vue-tsc --noEmit`
Expected: 本任务新增文件无 TS 错误（重点检查 `watch` 的 `immediate` 在 `props.open=false` 时不会触发 prepare；若触发，检查 `if (!value) return`）。

---

### Task 9: 三个入口接入

**Files:**
- Modify: `src/views/Project.vue`
- Modify: `src/views/RequestEditor.vue`

**Interfaces:**
- Consumes: `DocsPreviewDrawer`

- [ ] **Step 1: 项目页导入组件与状态**

`Project.vue` `<script setup>` 顶部加：

```ts
import DocsPreviewDrawer from '@/components/DocsPreviewDrawer.vue'
import type { DocScope } from '@/types/docs'
```

在 `const importingApi = ref(false)` 附近加：

```ts
const docsOpen = ref(false)
const docsScope = ref<DocScope>('project')
const docsTargetId = ref(0)

function openDocs(scope: DocScope, targetId: number) {
  docsScope.value = scope
  docsTargetId.value = targetId
  docsOpen.value = true
}
```

- [ ] **Step 2: 项目页菜单与集合操作**

导出菜单加一项：

```vue
<a-menu-item key="docs">接口文档（预览后导出）</a-menu-item>
```

`onExportMenu` 改为：

```ts
function onExportMenu({ key }: { key: string | number }) {
  if (key === 'project') void exportProject()
  else if (key === 'openapi') void exportOpenapi()
  else if (key === 'docs') openDocs('project', projectId.value)
}
```

集合节点操作区加：

```vue
<a @click="openDocs('collection', (node as TreeNode).raw.id)">文档</a>
```

并加在「+子集 / 改名 / 删除」之前。

- [ ] **Step 3: 项目页挂载抽屉**

在 `Project.vue` 模板最外层根节点结束前加：

```vue
<DocsPreviewDrawer
  v-model:open="docsOpen"
  :scope="docsScope"
  :target-id="docsTargetId"
  :project-id="projectId"
  :project-name="store.currentProject?.name"
/>
```

- [ ] **Step 4: 请求编辑页入口**

`RequestEditor.vue` 顶部加：

```ts
import DocsPreviewDrawer from '@/components/DocsPreviewDrawer.vue'
```

在 `envId` 附近加：

```ts
const docsOpen = ref(false)
```

在头部按钮组「代码」前加：

```vue
<a-button :disabled="!form.id" @click="docsOpen = true">
  <template #icon><FileTextOutlined /></template>
  文档
</a-button>
```

图标导入处补上：

```ts
import { FileTextOutlined } from '@ant-design/icons-vue'
```

在模板末尾（最后一个根闭合标签前）加：

```vue
<DocsPreviewDrawer
  v-model:open="docsOpen"
  scope="request"
  :target-id="form.id || 0"
  :project-id="projectId"
  :project-name="projectName"
  :default-environment-id="envId"
/>
```

- [ ] **Step 5: 构建检查**

Run: `npm run build`
Expected: `vue-tsc --noEmit && vite build` 成功；若 `FileTextOutlined` 已存在导入则复用，不要重复导入。

---

### Task 10: 端到端验证与收尾

**Files:**
- 无新增；只验证与修缺陷

- [ ] **Step 1: 跑 Rust 全部文档测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml docs:: -- --nocapture`
Expected: 全部 PASS

- [ ] **Step 2: 跑前端单测**

Run: `npm test -- docsExport`
Expected: 全部 PASS

- [ ] **Step 3: 跑前端构建**

Run: `npm run build`
Expected: exit code 0

- [ ] **Step 4: 起开发环境手工验证**

Run: `npm run tauri dev`

依次验证：

1. 项目页 → 导出 → 「接口文档（预览后导出）」，抽屉打开后自动出现 HTML 预览；
2. 切换环境，URL 变为真实域名，敏感 env 变量显示 `••••••`；
3. 编辑文档标题、接口说明、一个 query 示例值，右侧预览 300ms 内更新；
4. 关闭抽屉再打开，编辑仍在；点「重新生成」需二次确认，确认后编辑清空；
5. 导出 Markdown 与 HTML，文件落盘、内容与预览一致；
6. 全局搜索真实 token / 密码 / `tok-secret`，零命中；
7. 集合节点「文档」入口作用于该集合子树；请求编辑页「文档」入口带当前环境且只含该接口。

- [ ] **Step 5: 记录未覆盖项**

在最终回复里明确：PDF、OpenAPI 三级、DB v9 描述字段、AI 工具、CLI 属于阶段二/三，本次未实现；本机未逐一验证 2000 接口大项目（单测覆盖载荷上限逻辑）。

---

## 自检

- 规格覆盖：4.1 采集、4.2/4.2.1 脱敏、4.4 环境解析、4.5 示例链、5.1 模块、5.2/5.2.1 IR 与覆盖层、5.3 三个命令、5.4 流程、5.5 上限、第 6 章前端、第 7 章边界、8.1/8.2/8.3 测试、9 风险、10 阶段一 A/B、11 的 D-01~D-14/D-18~D-20 均有对应任务。
- 明确后置：PDF（5.7）、OpenAPI 重构（5.6）、DB v9 描述字段、阶段三。
- 类型一致性：前端 `DocOutlineRequest.params` 与 Rust `DocOutlineRequest.params` 一致；覆盖层 `meta.nodes.excluded.order` 与 Rust 一致；`source` vs `example_source` 命名按设计：大纲用 `source`，IR 用 `example_source`。
- 无提交步骤：遵循用户偏好，由用户统一提交。
