//! 记事本 IPC 薄封装：只做参数解析、调用领域层、发变更事件。

use tauri::{AppHandle, Emitter, State};

use crate::notes::attachment::{self, FileData, NoteFile};
use crate::notes::vision::{self, ExtractResult};
use crate::notes::{
    self, CreateFromImageResult, NoteCounts, NoteDetail, NoteInput, NotePatch, NotesFilter,
    NotesListResult, NotesSettings, TagCount, ToggleResult,
};
use crate::state::AppState;

fn emit_changed(app: &AppHandle, reason: &str, id: Option<i64>) {
    let payload = serde_json::json!({ "reason": reason, "id": id });
    if let Err(error) = app.emit("notes:changed", payload) {
        log::warn!("发送待办变更事件失败：{error}");
    }
}

#[tauri::command]
pub fn notes_list(
    state: State<'_, AppState>,
    filter: Option<NotesFilter>,
) -> Result<NotesListResult, String> {
    notes::list(&state.pool, &filter.unwrap_or_default())
}

#[tauri::command]
pub fn notes_get(state: State<'_, AppState>, id: i64) -> Result<NoteDetail, String> {
    notes::get(&state.pool, id)
}

#[tauri::command]
pub fn notes_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NoteInput,
) -> Result<NoteDetail, String> {
    let note = notes::create(&state.pool, input)?;
    emit_changed(&app, "create", Some(note.summary.id));
    Ok(note)
}

#[tauri::command]
pub fn notes_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    patch: NotePatch,
) -> Result<NoteDetail, String> {
    let note = notes::update(&state.pool, id, patch)?;
    emit_changed(&app, "update", Some(id));
    Ok(note)
}

#[tauri::command]
pub fn notes_toggle(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    done: bool,
) -> Result<ToggleResult, String> {
    let result = notes::toggle(&state.pool, id, done)?;
    emit_changed(&app, "toggle", Some(id));
    Ok(result)
}

#[tauri::command]
pub fn notes_snooze(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    minutes: i64,
) -> Result<NoteDetail, String> {
    let note = notes::snooze(&state.pool, id, minutes)?;
    emit_changed(&app, "update", Some(id));
    Ok(note)
}

#[tauri::command]
pub fn notes_delete(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    notes::delete(&state.pool, id)?;
    emit_changed(&app, "delete", Some(id));
    Ok(())
}

#[tauri::command]
pub fn notes_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<NoteDetail, String> {
    let note = notes::restore(&state.pool, id)?;
    emit_changed(&app, "restore", Some(id));
    Ok(note)
}

#[tauri::command]
pub fn notes_clear_done(app: AppHandle, state: State<'_, AppState>) -> Result<i64, String> {
    let count = notes::clear_done(&state.pool)?;
    emit_changed(&app, "delete", None);
    Ok(count)
}

#[tauri::command]
pub fn notes_counts(state: State<'_, AppState>) -> Result<NoteCounts, String> {
    notes::counts(&state.pool)
}

#[tauri::command]
pub fn notes_tags(state: State<'_, AppState>) -> Result<Vec<TagCount>, String> {
    notes::tags(&state.pool)
}

#[tauri::command]
pub fn notes_settings_get(state: State<'_, AppState>) -> NotesSettings {
    notes::get_settings(&state.pool)
}

#[tauri::command]
pub fn notes_settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: NotesSettings,
) -> Result<NotesSettings, String> {
    let saved = notes::set_settings(&state.pool, &settings)?;
    if let Err(error) = notes::quick::sync_shortcut(&app, saved.global_shortcut) {
        // 注册失败时把开关落回关闭，避免界面显示「已开启」但实际没有热键
        let mut fallback = saved.clone();
        fallback.global_shortcut = false;
        let _ = notes::set_settings(&state.pool, &fallback);
        emit_changed(&app, "settings", None);
        return Err(error);
    }
    emit_changed(&app, "settings", None);
    Ok(saved)
}

#[tauri::command]
pub fn notes_quick_show(app: AppHandle) {
    notes::quick::show(&app);
}

#[tauri::command]
pub fn notes_attachment_add(
    app: AppHandle,
    state: State<'_, AppState>,
    note_id: i64,
    file_name: Option<String>,
    mime: Option<String>,
    base64: Option<String>,
    path: Option<String>,
) -> Result<NoteFile, String> {
    let file = attachment::add(&state.pool, note_id, file_name, mime, base64, path)?;
    emit_changed(&app, "update", Some(note_id));
    Ok(file)
}

#[tauri::command]
pub fn notes_attachment_list(
    state: State<'_, AppState>,
    note_id: i64,
) -> Result<Vec<NoteFile>, String> {
    attachment::list(&state.pool, note_id)
}

#[tauri::command]
pub fn notes_attachment_data(state: State<'_, AppState>, file_id: i64) -> Result<FileData, String> {
    attachment::data(&state.pool, file_id)
}

#[tauri::command]
pub fn notes_attachment_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    note_id: i64,
    file_id: i64,
) -> Result<(), String> {
    attachment::remove(&state.pool, note_id, file_id)?;
    emit_changed(&app, "update", Some(note_id));
    Ok(())
}

#[tauri::command]
pub fn notes_attachment_save(
    state: State<'_, AppState>,
    file_id: i64,
    path: String,
) -> Result<(), String> {
    attachment::save(&state.pool, file_id, &path)
}

#[tauri::command]
pub async fn notes_extract_image(
    state: State<'_, AppState>,
    provider_id: Option<i64>,
    mime: String,
    base64: String,
    request_id: String,
) -> Result<ExtractResult, String> {
    vision::extract(&state, provider_id, &mime, &base64, &request_id).await
}

#[tauri::command]
pub fn notes_extract_cancel(state: State<'_, AppState>, request_id: String) -> bool {
    state.ai_request_cancel(&request_id)
}

#[tauri::command]
pub fn notes_create_from_image(
    app: AppHandle,
    state: State<'_, AppState>,
    file_name: Option<String>,
    mime: Option<String>,
    base64: Option<String>,
    path: Option<String>,
    tasks: Vec<NoteInput>,
) -> Result<CreateFromImageResult, String> {
    let result = notes::create_from_image(&state.pool, file_name, mime, base64, path, tasks)?;
    emit_changed(&app, "create", None);
    Ok(result)
}
