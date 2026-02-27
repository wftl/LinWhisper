//! Global hotkey handling for recording toggle

use crate::error::{AppError, Result};
use crate::state::{RecordingStatus, SharedState};
use crate::tray::{update_tray_icon, update_tray_icon_for_level, update_tray_menu};
use log::info;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Default hotkey for toggling recording
pub const DEFAULT_HOTKEY: &str = "Ctrl+Space";

/// Set up the global hotkey for recording toggle
pub fn setup_hotkey(app: &tauri::App) -> Result<()> {
    let handle = app.handle().clone();

    // Parse the shortcut
    let shortcut: Shortcut = DEFAULT_HOTKEY.parse()
        .map_err(|e| crate::error::AppError::Config(format!("Invalid hotkey: {}", e)))?;

    info!("Registering global hotkey: {}", DEFAULT_HOTKEY);

    // Wire up the handler (plugin already registered in Builder)
    app.global_shortcut()
        .on_shortcut(shortcut.clone(), move |_app, _shortcut_ref, event| {
            if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                info!("Hotkey pressed");
                toggle_recording(&handle);
            }
        })
        .map_err(|e| AppError::Config(format!("Failed to set hotkey handler: {}", e)))?;

    info!("Global hotkey registered successfully");
    Ok(())
}

/// Toggle recording state
fn toggle_recording(handle: &AppHandle) {
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state_arc) = handle.try_state::<SharedState>() {
            // Check recording state with minimal lock time
            let is_recording = {
                let state = state_arc.lock().await;
                state.is_recording()
            };

            if is_recording {
                // Stop recording - get data quickly, then release lock for processing
                let _ = crate::indicator::hide_indicator(&handle);
                let stop_result = {
                    let mut state = state_arc.lock().await;
                    let _ = update_tray_icon(&handle, RecordingStatus::Processing);
                    state.stop_recording().await
                };

                match stop_result {
                    Ok(output) => {
                        info!("Recording stopped via hotkey. Output: {} chars", output.len());
                        let _ = update_tray_icon(&handle, RecordingStatus::Ready);
                        let state = state_arc.lock().await;
                        let _ = update_tray_menu(&handle, &state).await;
                    }
                    Err(e) => {
                        log::error!("Failed to stop recording: {}", e);
                        let _ = update_tray_icon(&handle, RecordingStatus::Error);
                    }
                }
            } else {
                // Start recording with level callback for tray icon updates
                let handle_for_callback = handle.clone();
                let level_callback: crate::audio::LevelCallback = Box::new(move |level| {
                    let _ = update_tray_icon_for_level(&handle_for_callback, level);
                    crate::indicator::emit_audio_level(&handle_for_callback, level, level);
                });

                let start_result = {
                    let mut state = state_arc.lock().await;
                    let result = state.start_recording_with_callback(Some(level_callback));
                    if result.is_ok() {
                        let _ = update_tray_icon(&handle, RecordingStatus::Recording);
                        let _ = update_tray_menu(&handle, &state).await;
                    }
                    result
                };

                match start_result {
                    Ok(()) => {
                        let _ = crate::indicator::show_indicator(&handle);
                        info!("Recording started via hotkey");
                    }
                    Err(e) => {
                        log::error!("Failed to start recording: {}", e);
                        let _ = update_tray_icon(&handle, RecordingStatus::Error);
                    }
                }
            }
        }
    });
}
