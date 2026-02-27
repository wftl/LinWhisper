//! WhisperTray - A tray-based dictation tool
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
use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

/// Register WhisperTray to launch at Windows login.
///
/// Uses HKCU so no UAC prompt is required.
/// Only runs in release builds — dev runs are skipped so the registry
/// never points at a temporary debug binary.
#[cfg(windows)]
fn register_startup() {
    #[cfg(not(debug_assertions))]
    {
        if let Ok(exe) = std::env::current_exe() {
            let exe_str = exe.to_string_lossy();
            match std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "WhisperTray",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{}\"", exe_str),
                    "/f",
                ])
                .output()
            {
                Ok(out) if out.status.success() => {
                    log::info!("WhisperTray registered for startup at login");
                }
                Ok(out) => {
                    log::warn!(
                        "Startup registration failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                Err(e) => log::warn!("Startup registration error: {}", e),
            }
        }
    }
    // debug builds: do nothing
    #[cfg(debug_assertions)]
    log::debug!("Startup registration skipped in debug build");
}

#[cfg(not(windows))]
fn register_startup() {} // no-op on Linux/macOS

/// Initialize and run the Tauri application
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("Starting WhisperTray...");

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            info!("Setting up application...");

            // Initialize application state
            let state = Arc::new(Mutex::new(AppState::new(app.handle().clone())?));

            // Store state in app
            app.manage(state.clone());

            // Set up system tray
            tray::setup_tray(app)?;

            // Set up global hotkey (Ctrl+Space by default)
            if let Err(e) = hotkey::setup_hotkey(app) {
                log::error!("Failed to set up global hotkey: {}", e);
            }

            // Load modes
            let app_handle = app.handle().clone();
            let state_clone = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut state = state_clone.lock().await;
                if let Err(e) = state.load_modes().await {
                    log::error!("Failed to load modes: {}", e);
                }
                // Update tray menu with loaded modes
                if let Err(e) = tray::update_tray_menu(&app_handle, &state).await {
                    log::error!("Failed to update tray menu: {}", e);
                }
            });

            // Initialize database
            let state_clone = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut state = state_clone.lock().await;
                if let Err(e) = state.init_database().await {
                    log::error!("Failed to initialize database: {}", e);
                }
            });

            // Register for Windows startup (release builds only)
            register_startup();

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
            commands::save_api_key,
            commands::delete_api_key,
            commands::has_api_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
