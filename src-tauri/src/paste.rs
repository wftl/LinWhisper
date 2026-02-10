//! Clipboard and paste simulation module
//!
//! Supports multiple backends:
//! - X11: enigo (libxdo)
//! - Wayland: wtype or ydotool
//! - Fallback: clipboard only

use crate::error::{AppError, Result};
use arboard::Clipboard;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Paste backend detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteBackend {
    /// X11 with enigo/libxdo
    Enigo,
    /// Wayland with wtype
    Wtype,
    /// Wayland/X11 with ydotool
    Ydotool,
    /// No paste simulation available, clipboard only
    ClipboardOnly,
}

/// Detect the best available paste backend
pub fn detect_backend() -> PasteBackend {
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
        // On X11, use enigo (libxdo)
        log::info!("Paste backend: enigo (X11)");
        PasteBackend::Enigo
    }
}

/// Check if a command is available in PATH
fn is_command_available(cmd: &str) -> bool {
    Command::new("which")
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
                paste(false)?;
            }
        } else {
            // On X11, detect if the focused window is a terminal emulator.
            // Terminals use Ctrl+Shift+V for paste instead of Ctrl+V.
            let terminal = get_active_window_class()
                .map(|class| {
                    let is_term = is_terminal_window(&class);
                    if is_term {
                        log::info!(
                            "Terminal detected (class: {}), will use Ctrl+Shift+V",
                            class
                        );
                    } else {
                        log::debug!("Active window class: {}", class);
                    }
                    is_term
                })
                .unwrap_or(false);
            paste(terminal)?;
        }
    }

    Ok(())
}

/// Simulate paste using the best available backend.
/// If `shifted` is true, sends Ctrl+Shift+V (for terminal emulators)
/// instead of the default Ctrl+V.
fn paste(shifted: bool) -> Result<()> {
    let backend = detect_backend();

    // Delay to ensure clipboard is ready and user has released hotkey
    thread::sleep(Duration::from_millis(200));

    match backend {
        PasteBackend::Enigo => paste_enigo(shifted),
        PasteBackend::Wtype => {
            // Try wtype, fall back to ydotool if it fails (compositor may not support virtual keyboard)
            if let Err(e) = paste_wtype(shifted) {
                log::warn!("wtype failed ({}), trying ydotool fallback", e);
                if is_command_available("ydotool") {
                    paste_ydotool(shifted)
                } else {
                    log::warn!("No fallback available, text is in clipboard");
                    Ok(())
                }
            } else {
                Ok(())
            }
        }
        PasteBackend::Ydotool => paste_ydotool(shifted),
        PasteBackend::ClipboardOnly => {
            log::info!("No paste backend available, text is in clipboard");
            Ok(())
        }
    }
}

/// Paste using enigo (X11/libxdo)
fn paste_enigo(shifted: bool) -> Result<()> {
    use enigo::{Enigo, Keyboard, Settings};

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| AppError::Clipboard(format!("Failed to create input simulator: {}", e)))?;

    // Simulate Ctrl+V or Ctrl+Shift+V
    enigo
        .key(enigo::Key::Control, enigo::Direction::Press)
        .map_err(|e| AppError::Clipboard(format!("Failed to press Ctrl: {}", e)))?;

    if shifted {
        enigo
            .key(enigo::Key::Shift, enigo::Direction::Press)
            .map_err(|e| AppError::Clipboard(format!("Failed to press Shift: {}", e)))?;
    }

    thread::sleep(Duration::from_millis(20));

    enigo
        .key(enigo::Key::Unicode('v'), enigo::Direction::Click)
        .map_err(|e| AppError::Clipboard(format!("Failed to press V: {}", e)))?;

    thread::sleep(Duration::from_millis(20));

    if shifted {
        enigo
            .key(enigo::Key::Shift, enigo::Direction::Release)
            .map_err(|e| AppError::Clipboard(format!("Failed to release Shift: {}", e)))?;
    }

    enigo
        .key(enigo::Key::Control, enigo::Direction::Release)
        .map_err(|e| AppError::Clipboard(format!("Failed to release Ctrl: {}", e)))?;

    log::info!(
        "Paste completed (enigo/X11{})",
        if shifted { ", Ctrl+Shift+V" } else { "" }
    );
    Ok(())
}

/// Paste using wtype (Wayland)
fn paste_wtype(shifted: bool) -> Result<()> {
    let output = if shifted {
        // wtype -M ctrl -M shift -k v -m shift -m ctrl
        Command::new("wtype")
            .args(["-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"])
            .output()
    } else {
        // wtype -M ctrl -k v -m ctrl
        Command::new("wtype")
            .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
            .output()
    }
    .map_err(|e| AppError::Clipboard(format!("Failed to run wtype: {}", e)))?;

    if output.status.success() {
        log::info!(
            "Paste completed (wtype/Wayland{})",
            if shifted { ", Ctrl+Shift+V" } else { "" }
        );
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
fn paste_ydotool(shifted: bool) -> Result<()> {
    let key_combo = if shifted { "ctrl+shift+v" } else { "ctrl+v" };
    let output = Command::new("ydotool")
        .args(["key", key_combo])
        .output()
        .map_err(|e| AppError::Clipboard(format!("Failed to run ydotool: {}", e)))?;

    if output.status.success() {
        log::info!(
            "Paste completed (ydotool{})",
            if shifted { ", Ctrl+Shift+V" } else { "" }
        );
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
            Err(AppError::Clipboard("No typing backend available".to_string()))
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

/// Get the WM_CLASS of the currently focused window (X11 only).
/// Uses xdotool to find the active window ID, then xprop to read WM_CLASS.
/// Returns the full WM_CLASS string (e.g. "gnome-terminal-server", "Gnome-terminal")
/// so that is_terminal_window() can substring-match against it.
fn get_active_window_class() -> Option<String> {
    // Get active window ID via xdotool
    let window_id = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

    // Read WM_CLASS via xprop
    // Output format: WM_CLASS(STRING) = "instance", "class"
    let output = Command::new("xprop")
        .args(["-id", &window_id, "WM_CLASS"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

    if output.contains("WM_CLASS") {
        Some(output)
    } else {
        None
    }
}

/// Check if a window class name belongs to a terminal emulator.
/// Uses case-insensitive substring matching against known terminal classes.
fn is_terminal_window(class: &str) -> bool {
    let class_lower = class.to_lowercase();

    // Substrings that reliably identify terminal emulators.
    // "terminal" alone catches gnome-terminal, xfce4-terminal, mate-terminal,
    // lxterminal, elementary-terminal, deepin-terminal, etc.
    const TERMINAL_SUBSTRINGS: &[&str] = &[
        "terminal",
        "konsole",
        "alacritty",
        "kitty",
        "xterm",
        "rxvt",       // urxvt, rxvt-unicode
        "terminator",
        "tilix",
        "foot",
        "wezterm",
        "termite",
        "tilda",
        "guake",
        "yakuake",
        "sakura",
        "terminology",
        "cool-retro-term",
        "ghostty",
        "contour",
        "warp",
    ];

    TERMINAL_SUBSTRINGS
        .iter()
        .any(|substr| class_lower.contains(substr))
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
            PasteBackend::Enigo => "Using enigo (X11). Full paste simulation supported.".to_string(),
            PasteBackend::Wtype => "Using wtype (Wayland). Full paste simulation supported.".to_string(),
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
    fn test_detect_backend() {
        let backend = detect_backend();
        // Just ensure it returns something valid
        assert!(matches!(
            backend,
            PasteBackend::Enigo
                | PasteBackend::Wtype
                | PasteBackend::Ydotool
                | PasteBackend::ClipboardOnly
        ));
    }

    #[test]
    fn test_get_paste_info() {
        let info = get_paste_info();
        assert!(info.clipboard_supported);
    }

    #[test]
    fn test_is_terminal_window() {
        // Should match common terminals
        assert!(is_terminal_window("Gnome-terminal"));
        assert!(is_terminal_window("gnome-terminal-server"));
        assert!(is_terminal_window("xfce4-terminal"));
        assert!(is_terminal_window("Alacritty"));
        assert!(is_terminal_window("kitty"));
        assert!(is_terminal_window("konsole"));
        assert!(is_terminal_window("Tilix"));
        assert!(is_terminal_window("foot"));
        assert!(is_terminal_window("org.wezfurlong.wezterm"));

        // Should NOT match non-terminals
        assert!(!is_terminal_window("Firefox"));
        assert!(!is_terminal_window("code"));
        assert!(!is_terminal_window("Nautilus"));
        assert!(!is_terminal_window("libreoffice"));
    }
}
