//! Tauri command handlers

use crate::audio::{get_input_devices as get_audio_devices, AudioDevice};
use crate::database::HistoryItem;
use crate::modes::Mode;
use crate::state::{RecordingStatus, Settings, SharedState};
use crate::tray::{update_tray_icon, update_tray_menu};
use serde::{Deserialize, Serialize};
use tauri::State;

/// Recording status response
#[derive(Debug, Serialize)]
pub struct RecordingStatusResponse {
    pub status: RecordingStatus,
    pub is_recording: bool,
}

/// Start recording
#[tauri::command]
pub async fn start_recording(
    state: State<'_, SharedState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut state = state.lock().await;

    state.start_recording().map_err(|e| e.to_string())?;
    update_tray_icon(&app_handle, RecordingStatus::Recording).map_err(|e| e.to_string())?;
    update_tray_menu(&app_handle, &state)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Stop recording and get the result
#[tauri::command]
pub async fn stop_recording(
    state: State<'_, SharedState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let mut state = state.lock().await;

    update_tray_icon(&app_handle, RecordingStatus::Processing).map_err(|e| e.to_string())?;

    let result = state.stop_recording().await;

    // We need to update tray here to match the reset to Ready state, because GUI button path doesn't emit events (frontend handles its own error).
    let _ = update_tray_icon(&app_handle, state.status);
    let _ = update_tray_menu(&app_handle, &state).await;

    result.map_err(|e| e.to_string())
}

/// Get current recording status
#[tauri::command]
pub async fn get_recording_status(
    state: State<'_, SharedState>,
) -> Result<RecordingStatusResponse, String> {
    let state = state.lock().await;

    Ok(RecordingStatusResponse {
        status: state.status,
        is_recording: state.is_recording(),
    })
}

/// Get all available modes
#[tauri::command]
pub async fn get_modes(state: State<'_, SharedState>) -> Result<Vec<Mode>, String> {
    let state = state.lock().await;
    Ok(state.modes.values().cloned().collect())
}

/// Set the active mode
#[tauri::command]
pub async fn set_active_mode(
    state: State<'_, SharedState>,
    app_handle: tauri::AppHandle,
    mode_key: String,
) -> Result<(), String> {
    let mut state = state.lock().await;

    state.set_active_mode(&mode_key).map_err(|e| e.to_string())?;
    update_tray_menu(&app_handle, &state)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Get the active mode
#[tauri::command]
pub async fn get_active_mode(state: State<'_, SharedState>) -> Result<Option<Mode>, String> {
    let state = state.lock().await;
    Ok(state.get_active_mode().cloned())
}

/// Get available input devices
#[tauri::command]
pub async fn get_input_devices() -> Result<Vec<AudioDevice>, String> {
    get_audio_devices().map_err(|e| e.to_string())
}

/// Set the input device
#[tauri::command]
pub async fn set_input_device(
    state: State<'_, SharedState>,
    app_handle: tauri::AppHandle,
    device_name: String,
) -> Result<(), String> {
    let mut state = state.lock().await;

    state.settings.input_device = device_name;
    state.save_settings().map_err(|e| e.to_string())?;

    update_tray_menu(&app_handle, &state)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Transcribe a file
#[tauri::command]
pub async fn transcribe_file(
    state: State<'_, SharedState>,
    app_handle: tauri::AppHandle,
    file_path: String,
) -> Result<String, String> {
    let state_guard = state.lock().await;

    update_tray_icon(&app_handle, RecordingStatus::Processing).map_err(|e| e.to_string())?;

    // Load audio from file
    let path = std::path::PathBuf::from(&file_path);
    let samples = crate::audio::load_wav(&path).map_err(|e| e.to_string())?;

    // Get active mode
    let mode = state_guard
        .get_active_mode()
        .cloned()
        .ok_or_else(|| "No active mode".to_string())?;

    let language = state_guard.settings.language.clone();
    drop(state_guard);

    // Transcribe
    let provider =
        crate::providers::stt::create_stt_provider(&mode.stt_provider, &mode.stt_model)
            .await
            .map_err(|e| e.to_string())?;

    let transcript = provider
        .transcribe(&samples, Some(&language))
        .await
        .map_err(|e| e.to_string())?;

    update_tray_icon(&app_handle, RecordingStatus::Ready).map_err(|e| e.to_string())?;

    Ok(transcript)
}

/// History query parameters
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub search: Option<String>,
}

/// Get history items
#[tauri::command]
pub async fn get_history(
    state: State<'_, SharedState>,
    query: Option<HistoryQuery>,
) -> Result<Vec<HistoryItem>, String> {
    let state = state.lock().await;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| "Database not initialized".to_string())?;

    let db = db.lock().unwrap();

    let query = query.unwrap_or(HistoryQuery {
        limit: Some(50),
        offset: Some(0),
        search: None,
    });

    if let Some(search) = &query.search {
        db.search_history(search, query.limit.unwrap_or(50))
            .map_err(|e| e.to_string())
    } else {
        db.get_history(query.limit.unwrap_or(50), query.offset.unwrap_or(0))
            .map_err(|e| e.to_string())
    }
}

/// Get a single history item
#[tauri::command]
pub async fn get_history_item(
    state: State<'_, SharedState>,
    id: String,
) -> Result<Option<HistoryItem>, String> {
    let state = state.lock().await;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| "Database not initialized".to_string())?;

    let db = db.lock().unwrap();
    db.get_history_item(&id).map_err(|e| e.to_string())
}

/// Reprocess a history item with a different mode
#[tauri::command]
pub async fn reprocess_history_item(
    state: State<'_, SharedState>,
    app_handle: tauri::AppHandle,
    id: String,
    mode_key: String,
) -> Result<String, String> {
    let state_guard = state.lock().await;

    update_tray_icon(&app_handle, RecordingStatus::Processing).map_err(|e| e.to_string())?;

    // Get history item
    let db = state_guard
        .database
        .as_ref()
        .ok_or_else(|| "Database not initialized".to_string())?;

    let item = {
        let db_guard = db.lock().unwrap();
        db_guard
            .get_history_item(&id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "History item not found".to_string())?
    };
    let mut item = item;

    // Get mode
    let mode = state_guard
        .modes
        .get(&mode_key)
        .cloned()
        .ok_or_else(|| "Mode not found".to_string())?;

    let language = state_guard.settings.language.clone();
    let api_key = state_guard.get_api_key(&mode.llm_provider).map_err(|e| e.to_string())?;
    drop(state_guard);

    // Reprocess
    let output = if mode.ai_processing && !mode.prompt_template.is_empty() {
        let provider = crate::providers::llm::create_llm_provider(
            &mode.llm_provider,
            &mode.llm_model,
            api_key.as_deref(),
        )
        .map_err(|e| e.to_string())?;

        let prompt = crate::modes::render_prompt(
            &mode.prompt_template,
            &item.transcript_raw,
            None,
            &language,
        );

        provider.complete(&prompt).await.map_err(|e| e.to_string())?
    } else {
        item.transcript_raw.clone()
    };

    // Update history item
    item.mode_key = mode_key;
    item.output_final = output.clone();
    item.llm_provider = if mode.ai_processing {
        Some(format!("{:?}", mode.llm_provider).to_lowercase())
    } else {
        None
    };
    item.llm_model = if mode.ai_processing {
        Some(mode.llm_model.clone())
    } else {
        None
    };

    let state_guard = state.lock().await;
    let db = state_guard
        .database
        .as_ref()
        .ok_or_else(|| "Database not initialized".to_string())?;
    {
        let db_guard = db.lock().unwrap();
        db_guard.update_history(&item).map_err(|e| e.to_string())?;
    }
    drop(state_guard);

    update_tray_icon(&app_handle, RecordingStatus::Ready).map_err(|e| e.to_string())?;

    Ok(output)
}

/// Delete a history item
#[tauri::command]
pub async fn delete_history_item(state: State<'_, SharedState>, id: String) -> Result<(), String> {
    let state = state.lock().await;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| "Database not initialized".to_string())?;

    let db_guard = db.lock().unwrap();

    // Get item to find audio file
    if let Some(item) = db_guard.get_history_item(&id).map_err(|e| e.to_string())? {
        // Delete audio file if exists
        if let Some(audio_path) = &item.audio_path {
            let _ = std::fs::remove_file(audio_path);
        }
    }

    db_guard.delete_history(&id).map_err(|e| e.to_string())
}

/// Export format options
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Txt,
    Md,
    Srt,
    Vtt,
}

/// Export a history item
#[tauri::command]
pub async fn export_history_item(
    state: State<'_, SharedState>,
    id: String,
    format: ExportFormat,
) -> Result<String, String> {
    let state = state.lock().await;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| "Database not initialized".to_string())?;

    let db_guard = db.lock().unwrap();

    let item = db_guard
        .get_history_item(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "History item not found".to_string())?;

    let content = match format {
        ExportFormat::Txt => item.output_final.clone(),
        ExportFormat::Md => {
            format!(
                "# Transcription\n\n**Date:** {}\n**Mode:** {}\n\n## Output\n\n{}",
                item.created_at.format("%Y-%m-%d %H:%M:%S"),
                item.mode_key,
                item.output_final
            )
        }
        ExportFormat::Srt => {
            // Simple SRT format (single segment)
            format!(
                "1\n00:00:00,000 --> 00:00:{:02},{:03}\n{}\n",
                item.duration_ms / 1000,
                item.duration_ms % 1000,
                item.output_final
            )
        }
        ExportFormat::Vtt => {
            // WebVTT format
            format!(
                "WEBVTT\n\n00:00:00.000 --> 00:00:{:02}.{:03}\n{}\n",
                item.duration_ms / 1000,
                item.duration_ms % 1000,
                item.output_final
            )
        }
    };

    Ok(content)
}

/// Get current settings
#[tauri::command]
pub async fn get_settings(state: State<'_, SharedState>) -> Result<Settings, String> {
    let state = state.lock().await;
    Ok(state.settings.clone())
}

/// Update settings
#[tauri::command]
pub async fn update_settings(
    state: State<'_, SharedState>,
    settings: Settings,
) -> Result<(), String> {
    let mut state = state.lock().await;
    state.settings = settings;
    state.save_settings().map_err(|e| e.to_string())
}

/// Save an API key
#[tauri::command]
pub async fn save_api_key(
    state: State<'_, SharedState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    let state = state.lock().await;
    state.save_api_key(&provider, &key).map_err(|e| e.to_string())
}

/// Delete an API key
#[tauri::command]
pub async fn delete_api_key(state: State<'_, SharedState>, provider: String) -> Result<(), String> {
    let state = state.lock().await;
    state.delete_api_key(&provider).map_err(|e| e.to_string())
}

/// Check if an API key exists
#[tauri::command]
pub async fn has_api_key(state: State<'_, SharedState>, provider: String) -> Result<bool, String> {
    let state = state.lock().await;
    Ok(state.has_api_key(&provider))
}
