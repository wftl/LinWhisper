//! WhisperTray - A tray-based dictation tool for desktop
//!
//! This application provides voice-to-text transcription with optional
//! AI post-processing, all accessible from the system tray.

pub mod audio;
pub mod commands;
pub mod database;
pub mod error;
pub mod hotkey;
pub mod indicator;
pub mod modes;
pub mod paste;
pub mod providers;
pub mod state;
pub mod tray;

use log::info;
use state::{AppState, SharedState};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

/// Initialize and run the Tauri application
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("Starting WhisperTray...");

    // Initialize managed application state before any window can invoke commands.
    let mut app_state = AppState::new().expect("failed to initialize app state");
    let first_run_onboarding = !app_state.settings.first_run_completed;
    if first_run_onboarding {
        app_state.settings.first_run_completed = true;
        if let Err(e) = app_state.save_settings() {
            log::error!("Failed to persist first-run completion flag: {}", e);
        }
    }

    tauri::Builder::default()
        .manage(SharedState(Mutex::new(app_state)))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(move |app| {
            info!("Setting up application...");

            // Set up system tray
            tray::setup_tray(app)?;

            // Set up global hotkey (Ctrl+Space by default)
            if let Err(e) = hotkey::setup_hotkey(app) {
                log::error!("Failed to set up global hotkey: {}", e);
            }

            // Load modes
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<crate::state::SharedState>();
                let mut state = state.0.lock().await;
                if let Err(e) = state.load_modes().await {
                    log::error!("Failed to load modes: {}", e);
                }
                let active_mode = state.get_active_mode().cloned();
                let cache = state.stt_provider_cache.clone();
                // Update tray menu with loaded modes
                if let Err(e) = tray::update_tray_menu(&app_handle, &state).await {
                    log::error!("Failed to update tray menu: {}", e);
                }
                drop(state);
                if let Some(mode) = active_mode {
                    cache.warm_installed(&mode.stt_provider, &mode.stt_model).await;
                }
            });

            // Initialize database
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<crate::state::SharedState>();
                let mut state = state.0.lock().await;
                if let Err(e) = state.init_database().await {
                    log::error!("Failed to initialize database: {}", e);
                }
            });

            // First-run onboarding: open app window directly to Settings once.
            if first_run_onboarding {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    let _ = app_handle.emit("navigate", "/settings");
                });
            }

            info!("Application setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_recording,
            commands::stop_recording,
            commands::get_recording_status,
            commands::get_modes,
            commands::set_active_mode,
            commands::get_active_mode,
            commands::get_input_devices,
            commands::set_input_device,
            commands::transcribe_file,
            commands::get_history,
            commands::get_history_item,
            commands::reprocess_history_item,
            commands::delete_history_item,
            commands::export_history_item,
            commands::get_settings,
            commands::update_settings,
            commands::preload_stt_model,
            commands::save_api_key,
            commands::delete_api_key,
            commands::has_api_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
