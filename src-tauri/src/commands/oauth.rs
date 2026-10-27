use serde::Deserialize;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2Config {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub scope: String,
}

/// 使用 Client Credentials 模式向 OAuth2 授权服务器申请访问令牌。
/// 只返回 access_token，方便前端填入认证配置后随请求发送。
#[tauri::command]
pub async fn oauth2_token(
    state: State<'_, AppState>,
    config: OAuth2Config,
    insecure: Option<bool>,
) -> Result<String, String> {
    let url = config.token_url.trim();
    if url.is_empty() {
        return Err("请填写 Token URL".to_string());
    }
    if config.client_id.trim().is_empty() {
        return Err("请填写 Client ID".to_string());
    }

    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "client_credentials".to_string()),
        ("client_id", config.client_id.trim().to_string()),
        ("client_secret", config.client_secret.trim().to_string()),
    ];
    if !config.scope.trim().is_empty() {
        form.push(("scope", config.scope.trim().to_string()));
    }

    let client = state.client(insecure.unwrap_or(false));
    let response = client
        .post(url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("请求 Token 接口失败：{e}"))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!(
            "获取令牌失败：HTTP {} {}",
            status.as_u16(),
            text.chars().take(300).collect::<String>()
        ));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Token 响应不是 JSON：{e}"))?;
    let token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            format!(
                "响应中没有 access_token：{}",
                text.chars().take(200).collect::<String>()
            )
        })?
        .to_string();

    Ok(token)
}
