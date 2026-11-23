//! Recording indicator window management

use crate::error::Result;
use log::info;
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};

const INDICATOR_LABEL: &str = "recording";
const INDICATOR_WIDTH: f64 = 288.0;
const INDICATOR_HEIGHT: f64 = 62.0;
const INDICATOR_TOP_Y: f64 = 12.0;

#[derive(Clone, Serialize)]
pub struct AudioLevel {
    pub level: f32,
    pub peak: f32,
}

/// Show the recording indicator window
pub fn show_indicator(handle: &AppHandle) -> Result<()> {
    let window = if let Some(window) = handle.get_webview_window(INDICATOR_LABEL) {
        window
    } else {
        WebviewWindowBuilder::new(
            handle,
            INDICATOR_LABEL,
            WebviewUrl::App("/recording".into()),
        )
        .title("")
        .inner_size(INDICATOR_WIDTH, INDICATOR_HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?
    };

    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| handle.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let size = LogicalSize::new(INDICATOR_WIDTH, INDICATOR_HEIGHT)
            .to_physical::<u32>(monitor.scale_factor());
        let x = monitor.position().x + (monitor.size().width as i32 - size.width as i32) / 2;
        let y = monitor.position().y + (INDICATOR_TOP_Y * monitor.scale_factor()).round() as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }

    window.show()?;
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
    let _ = handle.emit_to(INDICATOR_LABEL, "audio-level", AudioLevel { level, peak });
}

/// Emit processing state to the indicator
pub fn emit_processing(handle: &AppHandle, processing: bool) {
    let _ = handle.emit_to(INDICATOR_LABEL, "recording-processing", processing);
}
