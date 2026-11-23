//! System tray management

use crate::error::{AppError, Result};
use crate::indicator::{emit_audio_level, emit_processing, show_indicator};
use crate::state::{complete_recording, AppState, RecordingStatus, SharedState};
use log::info;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{image::Image, AppHandle, Emitter, Manager};

const TRAY_ID: &str = "main-tray";
const ICON_UNKNOWN_BUCKET: u8 = 255;
static LAST_LEVEL_BUCKET: AtomicU8 = AtomicU8::new(ICON_UNKNOWN_BUCKET);
static ICON_CACHE: OnceLock<HashMap<&'static str, Image<'static>>> = OnceLock::new();

/// Set up the system tray
pub fn setup_tray(app: &tauri::App) -> Result<()> {
    info!("Setting up system tray...");

    let handle = app.handle();

    // Build initial menu
    let menu = build_tray_menu(handle)?;

    // Load initial icon (green = ready)
    let icon = load_tray_icon("tray-green")?;

    // Create tray icon
    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip("WinWhisper - Click to record")
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // Toggle recording on left click
                handle_tray_click(tray.app_handle());
            }
        })
        .build(app)?;

    info!("System tray created");
    Ok(())
}

/// Build the tray menu
fn build_tray_menu(handle: &AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = MenuBuilder::new(handle)
        .item(&MenuItemBuilder::with_id("start_recording", "Start Recording").build(handle)?)
        .item(&MenuItemBuilder::with_id("stop_recording", "Stop Recording").build(handle)?)
        .separator()
        .item(
            &SubmenuBuilder::with_id(handle, "modes", "Mode")
                .item(
                    &MenuItemBuilder::with_id("mode_voice_to_text", "Voice to Text")
                        .build(handle)?,
                )
                .build()?,
        )
        .item(
            &SubmenuBuilder::with_id(handle, "devices", "Input Device")
                .item(&MenuItemBuilder::with_id("device_default", "Default").build(handle)?)
                .build()?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id("transcribe_file", "Transcribe File...").build(handle)?)
        .item(&MenuItemBuilder::with_id("history", "History...").build(handle)?)
        .item(&MenuItemBuilder::with_id("settings", "Settings...").build(handle)?)
        .separator()
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(handle)?)
        .build()?;

    Ok(menu)
}

/// Update the tray menu with current modes and devices
pub async fn update_tray_menu(handle: &AppHandle, state: &AppState) -> Result<()> {
    // Build modes submenu
    let mut modes_builder = SubmenuBuilder::with_id(handle, "modes", "Mode");

    for mode in state.modes.values() {
        let id = format!("mode_{}", mode.key);
        let label = if mode.key == state.active_mode_key {
            format!("✓ {}", mode.name)
        } else {
            mode.name.clone()
        };
        modes_builder = modes_builder.item(&MenuItemBuilder::with_id(&id, &label).build(handle)?);
    }

    let modes_menu = modes_builder.build()?;

    // Build devices submenu
    let devices = crate::audio::get_input_devices().unwrap_or_default();
    let mut devices_builder = SubmenuBuilder::with_id(handle, "devices", "Input Device");

    let current_device = &state.settings.input_device;

    // Add default option
    let default_label = if current_device.is_empty() {
        "✓ Default"
    } else {
        "Default"
    };
    devices_builder = devices_builder
        .item(&MenuItemBuilder::with_id("device_default", default_label).build(handle)?);

    for device in devices {
        let id = format!("device_{}", device.name.replace(' ', "_"));
        let label = if device.name == *current_device {
            format!("✓ {}", device.name)
        } else {
            device.name.clone()
        };
        devices_builder =
            devices_builder.item(&MenuItemBuilder::with_id(&id, &label).build(handle)?);
    }

    let devices_menu = devices_builder.build()?;

    // Rebuild menu
    let recording_label = if state.status == RecordingStatus::Recording {
        "Stop Recording"
    } else {
        "Start Recording"
    };

    let menu = MenuBuilder::new(handle)
        .item(&MenuItemBuilder::with_id("toggle_recording", recording_label).build(handle)?)
        .separator()
        .item(&modes_menu)
        .item(&devices_menu)
        .separator()
        .item(&MenuItemBuilder::with_id("transcribe_file", "Transcribe File...").build(handle)?)
        .item(&MenuItemBuilder::with_id("history", "History...").build(handle)?)
        .item(&MenuItemBuilder::with_id("settings", "Settings...").build(handle)?)
        .separator()
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(handle)?)
        .build()?;

    // Update tray menu
    if let Some(tray) = handle.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    }

    Ok(())
}

/// Update the tray icon based on status
pub fn update_tray_icon(handle: &AppHandle, status: RecordingStatus) -> Result<()> {
    // Force icon refresh on next level tick.
    LAST_LEVEL_BUCKET.store(ICON_UNKNOWN_BUCKET, Ordering::Relaxed);

    let icon_name = status.icon_name();
    let icon = load_tray_icon(icon_name)?;

    if let Some(tray) = handle.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(icon))?;

        let tooltip = match status {
            RecordingStatus::Loading => "WinWhisper - Loading model...",
            RecordingStatus::Recording => "WinWhisper - Recording...",
            RecordingStatus::Processing => "WinWhisper - Processing...",
            RecordingStatus::Ready => "WinWhisper - Ready (click to record)",
            RecordingStatus::Error => "WinWhisper - Error",
        };
        tray.set_tooltip(Some(tooltip))?;
    }

    Ok(())
}

/// Update the tray icon based on audio level (during recording)
/// level: 0.0 to 1.0
pub fn update_tray_icon_for_level(handle: &AppHandle, level: f32) -> Result<()> {
    // Map level to coarse buckets so we avoid redundant icon swaps.
    let (bucket, icon_name) = if level < 0.15 {
        (0u8, "tray-red")
    } else if level < 0.3 {
        (1u8, "tray-yellow")
    } else if level < 0.6 {
        (2u8, "tray-green")
    } else {
        (3u8, "tray-blue")
    };

    let previous = LAST_LEVEL_BUCKET.swap(bucket, Ordering::Relaxed);
    if previous == bucket {
        return Ok(());
    }

    let icon = load_tray_icon(icon_name)?;

    if let Some(tray) = handle.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(icon))?;
    }

    Ok(())
}

/// Load a tray icon by name
fn load_tray_icon(name: &str) -> Result<Image<'static>> {
    let cache = ICON_CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        let entries = [
            (
                "tray-yellow",
                include_bytes!("../icons/tray-yellow.png").as_slice(),
            ),
            (
                "tray-red",
                include_bytes!("../icons/tray-red.png").as_slice(),
            ),
            (
                "tray-blue",
                include_bytes!("../icons/tray-blue.png").as_slice(),
            ),
            (
                "tray-green",
                include_bytes!("../icons/tray-green.png").as_slice(),
            ),
        ];

        for (key, bytes) in entries {
            if let Ok(icon) = Image::from_bytes(bytes) {
                map.insert(key, icon);
            }
        }

        map
    });

    cache
        .get(name)
        .or_else(|| cache.get("tray-green"))
        .cloned()
        .ok_or_else(|| crate::error::AppError::Tauri("Tray icon cache is empty".to_string()))
}

/// Handle menu events
fn handle_menu_event(handle: &AppHandle, id: &str) {
    info!("Menu event: {}", id);

    match id {
        "toggle_recording" | "start_recording" => {
            handle_tray_click(handle);
        }
        "stop_recording" => {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = handle.try_state::<SharedState>() {
                    finish_tray_recording(&handle, &state).await;
                }
            });
        }
        "transcribe_file" => {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Open file dialog
                use tauri_plugin_dialog::DialogExt;
                handle
                    .dialog()
                    .file()
                    .add_filter("Audio Files", &["wav", "mp3", "m4a", "ogg", "flac"])
                    .pick_file(move |path| {
                        if let Some(path) = path {
                            info!("Selected file for transcription: {:?}", path);
                            // TODO: Implement file transcription
                        }
                    });
            });
        }
        "history" => {
            show_window(handle, "main");
            // Navigate to history view
            let _ = handle.emit("navigate", "/history");
        }
        "settings" => {
            show_window(handle, "main");
            // Navigate to settings view
            let _ = handle.emit("navigate", "/settings");
        }
        "quit" => {
            handle.exit(0);
        }
        _ => {
            // Handle mode selection
            if let Some(mode_key) = id.strip_prefix("mode_") {
                let handle = handle.clone();
                let mode_key = mode_key.to_string();
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = handle.try_state::<crate::state::SharedState>() {
                        let mut state = state.0.lock().await;
                        if let Err(e) = state.set_active_mode(&mode_key) {
                            log::error!("Failed to set mode: {}", e);
                        } else {
                            info!("Mode changed to: {}", mode_key);
                            let _ = update_tray_menu(&handle, &state).await;
                        }
                    }
                });
            }
            // Handle device selection
            else if let Some(device) = id.strip_prefix("device_") {
                let handle = handle.clone();
                let device_name = if device == "default" {
                    String::new()
                } else {
                    device.replace('_', " ")
                };
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = handle.try_state::<crate::state::SharedState>() {
                        let mut state = state.0.lock().await;
                        state.settings.input_device = device_name.clone();
                        if let Err(e) = state.save_settings() {
                            log::error!("Failed to save settings: {}", e);
                        } else {
                            info!("Input device changed to: {}", device_name);
                            let _ = update_tray_menu(&handle, &state).await;
                        }
                    }
                });
            }
        }
    }
}

/// Handle tray icon click (toggle recording)
fn handle_tray_click(handle: &AppHandle) {
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state) = handle.try_state::<SharedState>() {
            let (is_recording, is_processing) = {
                let state = state.0.lock().await;
                (state.is_recording(), state.status == RecordingStatus::Processing)
            };
            if is_processing {
                return;
            }

            if is_recording {
                finish_tray_recording(&handle, &state).await;
            } else {
                // Start recording
                let handle_for_callback = handle.clone();
                let level_callback: crate::audio::LevelCallback = Box::new(move |level, peak| {
                    let _ = update_tray_icon_for_level(&handle_for_callback, level);
                    emit_audio_level(&handle_for_callback, level, peak);
                });

                let mut state_guard = state.0.lock().await;
                match state_guard.start_recording_with_callback(Some(level_callback)) {
                    Ok(()) => {
                        info!("Recording started");
                        let _ = show_indicator(&handle);
                        emit_processing(&handle, false);
                        let _ = update_tray_icon(&handle, RecordingStatus::Recording);
                        let _ = update_tray_menu(&handle, &state_guard).await;

                        // Emit event to frontend
                        let _ = handle.emit("recording-started", ());
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

async fn finish_tray_recording(handle: &AppHandle, state: &SharedState) {
    match complete_recording(state, handle).await {
        Ok(output) => {
            info!("Recording stopped. Output: {} chars", output.len());
            let _ = handle.emit("recording-complete", &output);
        }
        Err(AppError::NoRecordingInProgress) => {}
        Err(e) => log::error!("Failed to stop recording: {}", e),
    }
}

/// Show a window
fn show_window(handle: &AppHandle, label: &str) {
    if let Some(window) = handle.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
