//! Recording indicator window management

use crate::error::Result;
use log::info;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const INDICATOR_LABEL: &str = "recording";

#[derive(Clone, Serialize)]
pub struct AudioLevel {
    pub level: f32,
    pub peak: f32,
}

/// Centre the indicator window at the top of the current monitor.
fn centre_indicator(window: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.current_monitor() {
        let screen = monitor.size();
        let scale = monitor.scale_factor();
        // Window logical size is 200×60; convert to physical pixels
        let pw = (200.0 * scale) as i32;
        let x = (screen.width as i32 - pw) / 2;
        let y = (50.0 * scale) as i32;
        let _ = window.set_position(tauri::Position::Physical(
            tauri::PhysicalPosition::new(x, y),
        ));
    }
}

/// Show the recording indicator window
pub fn show_indicator(handle: &AppHandle) -> Result<()> {
    // The window is pre-created by tauri.conf.json (visible: false).
    // If it somehow doesn't exist yet, create it on demand.
    let window = if let Some(w) = handle.get_webview_window(INDICATOR_LABEL) {
        w
    } else {
        WebviewWindowBuilder::new(
            handle,
            INDICATOR_LABEL,
            WebviewUrl::App("/recording".into()),
        )
        .title("")
        .inner_size(200.0, 60.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()?
    };

    // Always re-centre (position from tauri.conf.json is just a placeholder)
    centre_indicator(&window);

    let _ = window.show();
    let _ = window.set_focus();
    info!("Recording indicator shown");
    Ok(())
}

/// Hide the recording indicator window
pub fn hide_indicator(handle: &AppHandle) -> Result<()> {
    if let Some(window) = handle.get_webview_window(INDICATOR_LABEL) {
        let _ = window.hide();
        info!("Recording indicator hidden");
    }
    Ok(())
}

/// Emit an audio level update to the indicator
pub fn emit_audio_level(handle: &AppHandle, level: f32, peak: f32) {
    let _ = handle.emit_to(
        INDICATOR_LABEL,
        "audio-level",
        AudioLevel { level, peak },
    );
}

/// Emit processing state to the indicator
pub fn emit_processing(handle: &AppHandle, processing: bool) {
    let _ = handle.emit_to(INDICATOR_LABEL, "recording-processing", processing);
}
