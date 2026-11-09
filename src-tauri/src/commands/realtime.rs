//! 实时调试命令薄壳（G-09）：核心逻辑在 `services::realtime`。

use std::collections::HashMap;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

use crate::services::realtime::{self, RealtimeConnectPayload, RealtimeSink, RealtimeStatus};
use crate::state::AppState;
use crate::util::err;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeSendPayload {
    pub session_id: String,
    /// text 时是正文；binary 时是 Base64
    pub data: String,
    /// text（默认）| binary
    #[serde(default)]
    pub kind: Option<String>,
}

fn event_sink(app: AppHandle) -> RealtimeSink {
    Arc::new(move |event, data| {
        let _ = app.emit(event, data);
    })
}

#[tauri::command]
pub async fn realtime_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: RealtimeConnectPayload,
) -> Result<RealtimeStatus, String> {
    let vars = match payload.env_id {
        Some(env_id) => {
            let conn = state.pool.get().map_err(err)?;
            crate::commands::http::load_vars(&conn, env_id)?
        }
        None => HashMap::new(),
    };
    realtime::connect(
        &state.realtime,
        state.client(false),
        payload,
        vars,
        event_sink(app),
    )
    .await
}

#[tauri::command]
pub fn realtime_send(
    state: State<'_, AppState>,
    payload: RealtimeSendPayload,
) -> Result<(), String> {
    if payload.kind.as_deref() == Some("binary") {
        let data = B64
            .decode(payload.data.trim())
            .map_err(|_| "二进制消息不是合法 Base64".to_string())?;
        state.realtime.send_binary(&payload.session_id, data)
    } else {
        state.realtime.send_text(&payload.session_id, payload.data)
    }
}

#[tauri::command]
pub fn realtime_close(state: State<'_, AppState>, session_id: String) -> bool {
    state.realtime.close(&session_id)
}

#[tauri::command]
pub fn realtime_status(state: State<'_, AppState>, session_id: String) -> Option<RealtimeStatus> {
    state.realtime.status(&session_id)
}
