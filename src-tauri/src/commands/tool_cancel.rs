//! 通用工具取消（AI-02）。
//!
//! 停止按钮只停模型流时，`send_request` / `run_batch` 已经发出的 HTTP 仍在打服务。
//! 这里按 `trace_id` 统一中断在途请求：AI 发起请求时把 `trace_id` 作为 `requestKey`
//! 登记到 `http_cancels`，停止时从这里命中并置位。
//!
//! 压测 / 测试套件 / 抓包各有自己的停止命令，不走这里。

use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub fn tool_cancel(state: State<'_, AppState>, trace_id: String) -> bool {
    let id = trace_id.trim();
    if id.is_empty() {
        return false;
    }
    // 两个注册表都尝试；`|` 不做短路，避免漏掉另一张表
    state.http_cancel(id) | state.ai_request_cancel(id)
}
