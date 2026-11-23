//! Global hotkey handling for recording toggle

use crate::error::{AppError, Result};
use crate::indicator::{emit_audio_level, emit_processing, show_indicator};
use crate::state::{complete_recording, RecordingStatus, SharedState};
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
    let shortcut: Shortcut = DEFAULT_HOTKEY
        .parse()
        .map_err(|e| crate::error::AppError::Config(format!("Invalid hotkey: {}", e)))?;

    info!("Registering global hotkey: {}", DEFAULT_HOTKEY);

    // Register the shortcut
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |_app, shortcut_ref, event| {
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    info!("Hotkey pressed: {:?}", shortcut_ref);
                    toggle_recording(&handle);
                }
            })
            .build(),
    )?;

    // Register the specific shortcut
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| AppError::Config(format!("Failed to register hotkey: {}", e)))?;

    info!("Global hotkey registered successfully");
    Ok(())
}

/// Toggle recording state
fn toggle_recording(handle: &AppHandle) {
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state_arc) = handle.try_state::<SharedState>() {
            let (is_recording, is_processing) = {
                let state = state_arc.0.lock().await;
                (
                    state.is_recording(),
                    state.status == RecordingStatus::Processing,
                )
            };
            if is_processing {
                return;
            }

            if is_recording {
                match complete_recording(&state_arc, &handle).await {
                    Ok(output) => info!(
                        "Recording stopped via hotkey. Output: {} chars",
                        output.len()
                    ),
                    Err(AppError::NoRecordingInProgress) => {}
                    Err(e) => log::error!("Failed to stop recording: {}", e),
                }
            } else {
                // Start recording with level callback for tray icon updates
                let handle_for_callback = handle.clone();
                let level_callback: crate::audio::LevelCallback = Box::new(move |level, peak| {
                    let _ = update_tray_icon_for_level(&handle_for_callback, level);
                    emit_audio_level(&handle_for_callback, level, peak);
                });

                let start_result = {
                    let mut state = state_arc.0.lock().await;
                    let result = state.start_recording_with_callback(Some(level_callback));
                    if result.is_ok() {
                        let _ = show_indicator(&handle);
                        emit_processing(&handle, false);
                        let _ = update_tray_icon(&handle, RecordingStatus::Recording);
                        let _ = update_tray_menu(&handle, &state).await;
                    }
                    result
                };

                match start_result {
                    Ok(()) => {
                        info!("Recording started via hotkey");
                    }
                    Err(AppError::RecordingInProgress) => {}
                    Err(e) => {
                        log::error!("Failed to start recording: {}", e);
                        let _ = update_tray_icon(&handle, RecordingStatus::Error);
                    }
                }
            }
        }
    });
}
