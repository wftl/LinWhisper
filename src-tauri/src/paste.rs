//! Clipboard and paste simulation module
//!
//! Supports multiple backends:
//! - Windows: SendInput for paste, enigo for direct typing
//! - macOS/X11: enigo
//! - Wayland: wtype or ydotool
//! - Fallback: clipboard only

use crate::error::{AppError, Result};
use arboard::Clipboard;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

static CACHED_BACKEND: OnceLock<PasteBackend> = OnceLock::new();

/// Paste backend detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteBackend {
    /// Native Ctrl+V input on Windows
    #[cfg(target_os = "windows")]
    Windows,
    /// Cross-platform simulation with enigo
    #[cfg(not(target_os = "windows"))]
    Enigo,
    /// Wayland with wtype
    Wtype,
    /// Wayland/X11 with ydotool
    Ydotool,
    /// No paste simulation available, clipboard only
    ClipboardOnly,
}

/// Detect the best available paste backend (result is cached after first call)
pub fn detect_backend() -> PasteBackend {
    *CACHED_BACKEND.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            log::info!("Paste backend: SendInput (Windows)");
            PasteBackend::Windows
        }
        #[cfg(target_os = "macos")]
        {
            log::info!("Paste backend: enigo (macOS)");
            PasteBackend::Enigo
        }
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            if is_wayland() {
                // On Wayland, try wtype first, then ydotool
                if is_command_available("wtype") {
                    log::info!("Paste backend: wtype (Wayland)");
                    PasteBackend::Wtype
                } else if is_command_available("ydotool") {
                    log::info!("Paste backend: ydotool (Wayland)");
                    PasteBackend::Ydotool
                } else {
                    log::warn!("No Wayland paste backend available. Install wtype or ydotool for auto-paste.");
                    PasteBackend::ClipboardOnly
                }
            } else {
                // On X11 and other desktop sessions, use enigo
                log::info!("Paste backend: enigo");
                PasteBackend::Enigo
            }
        }
    })
}

/// Check if a command is available in PATH
fn is_command_available(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    let checker = "where";
    #[cfg(not(target_os = "windows"))]
    let checker = "which";

    Command::new(checker)
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Copy text to clipboard and optionally paste/type it
pub fn copy_and_paste(text: &str, should_paste: bool) -> Result<()> {
    // Copy to clipboard first (always useful as backup)
    let mut clipboard = Clipboard::new()
        .map_err(|e| AppError::Clipboard(format!("Failed to access clipboard: {}", e)))?;

    clipboard
        .set_text(text)
        .map_err(|e| AppError::Clipboard(format!("Failed to set clipboard text: {}", e)))?;

    log::info!("Text copied to clipboard ({} chars)", text.len());

    if should_paste {
        // On Wayland, prefer typing directly over Ctrl+V simulation
        // as it's more reliable across different compositors
        if is_wayland() {
            log::info!("Wayland detected, typing text directly");
            if let Err(e) = type_text(text) {
                log::warn!("Direct typing failed ({}), trying paste fallback", e);
                paste()?;
            }
        } else {
            paste()?;
        }
    }

    Ok(())
}

/// Simulate Ctrl+V paste using the best available backend
pub fn paste() -> Result<()> {
    let backend = detect_backend();

    #[cfg(not(target_os = "windows"))]
    thread::sleep(Duration::from_millis(200));

    match backend {
        #[cfg(target_os = "windows")]
        PasteBackend::Windows => paste_windows(),
        #[cfg(not(target_os = "windows"))]
        PasteBackend::Enigo => paste_enigo(),
        PasteBackend::Wtype => {
            // Try wtype, fall back to ydotool if it fails (compositor may not support virtual keyboard)
            if let Err(e) = paste_wtype() {
                log::warn!("wtype failed ({}), trying ydotool fallback", e);
                if is_command_available("ydotool") {
                    paste_ydotool()
                } else {
                    log::warn!("No fallback available, text is in clipboard");
                    Ok(())
                }
            } else {
                Ok(())
            }
        }
        PasteBackend::Ydotool => paste_ydotool(),
        PasteBackend::ClipboardOnly => {
            log::info!("No paste backend available, text is in clipboard");
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
fn paste_windows() -> Result<()> {
    use std::time::Instant;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SPACE, VK_V,
    };

    // Wait only when a key from the recording shortcut or another modifier is
    // physically down, so the paste goes to the focused app as an ordinary Ctrl+V.
    let deadline = Instant::now() + Duration::from_millis(250);
    let keys = [VK_CONTROL, VK_SPACE, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    while keys
        .iter()
        .any(|&key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
    {
        if Instant::now() >= deadline {
            return Err(AppError::Clipboard(
                "Paste keys are still held; text is on the clipboard".to_string(),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }

    let key = |code, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: code,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let input = [
        key(VK_CONTROL, 0),
        key(VK_V, 0),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let size = std::mem::size_of::<INPUT>() as i32;
    let sent = unsafe { SendInput(input.len() as u32, input.as_ptr(), size) };
    if sent != input.len() as u32 {
        if sent > 0 {
            // Release any keys that may have been pressed by a partial send.
            unsafe { SendInput(2, input[2..].as_ptr(), size) };
        }
        return Err(AppError::Clipboard(format!(
            "Windows sent {sent} of {} paste events",
            input.len()
        )));
    }
    Ok(())
}

/// Paste using enigo
#[cfg(not(target_os = "windows"))]
fn paste_enigo() -> Result<()> {
    use enigo::{Enigo, Keyboard, Settings};

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| AppError::Clipboard(format!("Failed to create input simulator: {}", e)))?;

    #[cfg(target_os = "macos")]
    let modifier_key = enigo::Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier_key = enigo::Key::Control;

    enigo
        .key(modifier_key, enigo::Direction::Press)
        .map_err(|e| AppError::Clipboard(format!("Failed to press modifier key: {}", e)))?;

    thread::sleep(Duration::from_millis(20));

    enigo
        .key(enigo::Key::Unicode('v'), enigo::Direction::Click)
        .map_err(|e| AppError::Clipboard(format!("Failed to press V: {}", e)))?;

    thread::sleep(Duration::from_millis(20));

    enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| AppError::Clipboard(format!("Failed to release modifier key: {}", e)))?;

    log::info!("Paste completed (enigo)");
    Ok(())
}

/// Paste using wtype (Wayland)
fn paste_wtype() -> Result<()> {
    // wtype -M ctrl -k v -m ctrl
    let output = Command::new("wtype")
        .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
        .output()
        .map_err(|e| AppError::Clipboard(format!("Failed to run wtype: {}", e)))?;

    if output.status.success() {
        log::info!("Paste completed (wtype/Wayland)");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Clipboard(format!(
            "wtype failed: {}",
            stderr.trim()
        )))
    }
}

/// Paste using ydotool (works on both X11 and Wayland)
fn paste_ydotool() -> Result<()> {
    // Use ydotool key with key names (works with newer versions)
    let output = Command::new("ydotool")
        .args(["key", "ctrl+v"])
        .output()
        .map_err(|e| AppError::Clipboard(format!("Failed to run ydotool: {}", e)))?;

    if output.status.success() {
        log::info!("Paste completed (ydotool)");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Clipboard(format!(
            "ydotool failed: {}",
            stderr.trim()
        )))
    }
}

/// Type text directly (alternative to paste for some applications)
pub fn type_text(text: &str) -> Result<()> {
    // Delay to ensure user has released hotkey and focus is correct
    thread::sleep(Duration::from_millis(200));

    let backend = detect_backend();

    match backend {
        #[cfg(target_os = "windows")]
        PasteBackend::Windows => type_text_enigo(text),
        #[cfg(not(target_os = "windows"))]
        PasteBackend::Enigo => type_text_enigo(text),
        PasteBackend::Wtype => {
            // Try wtype first, fall back to ydotool
            if let Err(e) = type_text_wtype(text) {
                log::warn!("wtype typing failed ({}), trying ydotool", e);
                if is_command_available("ydotool") {
                    type_text_ydotool(text)
                } else {
                    Err(e)
                }
            } else {
                Ok(())
            }
        }
        PasteBackend::Ydotool => type_text_ydotool(text),
        PasteBackend::ClipboardOnly => {
            log::info!("No type backend available");
            Err(AppError::Clipboard(
                "No typing backend available".to_string(),
            ))
        }
    }
}

/// Type text using enigo
fn type_text_enigo(text: &str) -> Result<()> {
    use enigo::{Enigo, Keyboard, Settings};

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| AppError::Clipboard(format!("Failed to create input simulator: {}", e)))?;

    enigo
        .text(text)
        .map_err(|e| AppError::Clipboard(format!("Failed to type text: {}", e)))?;

    log::info!("Text typed ({} chars) via enigo", text.len());
    Ok(())
}

/// Type text using wtype
fn type_text_wtype(text: &str) -> Result<()> {
    // wtype types text directly, use -d for delay between keys (ms)
    let output = Command::new("wtype")
        .args(["-d", "0", text])
        .output()
        .map_err(|e| AppError::Clipboard(format!("Failed to run wtype: {}", e)))?;

    if output.status.success() {
        log::info!("Text typed ({} chars) via wtype", text.len());
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Clipboard(format!(
            "wtype type failed: {}",
            stderr.trim()
        )))
    }
}

/// Type text using ydotool
fn type_text_ydotool(text: &str) -> Result<()> {
    // Use --delay 0 to start immediately (we handle delay ourselves)
    // Use --key-delay for reasonable typing speed
    let output = Command::new("ydotool")
        .args(["type", "--delay", "50", "--key-delay", "0", "--", text])
        .output()
        .map_err(|e| AppError::Clipboard(format!("Failed to run ydotool: {}", e)))?;

    if output.status.success() {
        log::info!("Text typed ({} chars) via ydotool", text.len());
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Clipboard(format!(
            "ydotool type failed: {}",
            stderr.trim()
        )))
    }
}

/// Get text from clipboard
pub fn get_clipboard_text() -> Result<String> {
    let mut clipboard = Clipboard::new()
        .map_err(|e| AppError::Clipboard(format!("Failed to access clipboard: {}", e)))?;

    clipboard
        .get_text()
        .map_err(|e| AppError::Clipboard(format!("Failed to get clipboard text: {}", e)))
}

/// Check if we're running under Wayland
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}

/// Get information about paste capabilities
pub fn get_paste_info() -> PasteInfo {
    let is_wayland = is_wayland();
    let backend = detect_backend();

    PasteInfo {
        is_wayland,
        backend,
        paste_supported: backend != PasteBackend::ClipboardOnly,
        type_supported: backend != PasteBackend::ClipboardOnly,
        clipboard_supported: true,
        notes: match backend {
            #[cfg(target_os = "windows")]
            PasteBackend::Windows => {
                "Using Windows SendInput. Paste and typing are supported.".to_string()
            }
            #[cfg(not(target_os = "windows"))]
            PasteBackend::Enigo => "Using enigo. Full paste simulation supported.".to_string(),
            PasteBackend::Wtype => {
                "Using wtype (Wayland). Full paste simulation supported.".to_string()
            }
            PasteBackend::Ydotool => "Using ydotool. Full paste simulation supported.".to_string(),
            PasteBackend::ClipboardOnly => {
                if is_wayland {
                    "Wayland detected but no paste backend available. Install wtype or ydotool for auto-paste. Text is copied to clipboard.".to_string()
                } else {
                    "No paste backend available. Text is copied to clipboard.".to_string()
                }
            }
        },
    }
}

/// Information about paste capabilities
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PasteInfo {
    pub is_wayland: bool,
    #[serde(skip)]
    pub backend: PasteBackend,
    pub paste_supported: bool,
    pub type_supported: bool,
    pub clipboard_supported: bool,
    pub notes: String,
}

impl Default for PasteBackend {
    fn default() -> Self {
        PasteBackend::ClipboardOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_wayland_detection() {
        // This test just ensures the function doesn't panic
        let _ = is_wayland();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_uses_native_paste() {
        assert_eq!(detect_backend(), PasteBackend::Windows);
    }

    #[test]
    fn test_get_paste_info() {
        let info = get_paste_info();
        assert!(info.clipboard_supported);
    }
}
