# WinWhisper

WinWhisper is a Windows-native port of LinWhisper: a local-first, tray-based dictation app with optional AI post-processing.

## What Is Implemented

- Windows packaging targets (`nsis`, `msi`) in Tauri config
- Cross-platform paste backend detection with Windows support
- PowerShell helper scripts for run/build workflows
- Existing local STT (`whisper.cpp` via `whisper-rs`), history, tray, and hotkey flows retained

## Prerequisites (Windows)

- Node.js 20+
- Rust toolchain (`rustup` + `cargo`)
- Microsoft Visual Studio C++ Build Tools
- WebView2 runtime (usually preinstalled on Windows 11)

## Quick Start

```powershell
# In project root
npm install
npm run tauri dev
```

## Build Windows Installer

```powershell
npm run tauri build
```

Artifacts are created under:

- `src-tauri/target/release/bundle/nsis/`
- `src-tauri/target/release/bundle/msi/`

## PowerShell Helpers

```powershell
# Dev run
.\run.ps1

# Production run
.\run.ps1 prod

# Debug build
.\build.ps1

# Release build
.\build.ps1 release
```

## Notes

- The default global hotkey is `Ctrl+Space`.
- On Windows, auto-paste sends Ctrl+V as a single batch after the recording shortcut is released.
- WinWhisper warms the active speech model in the background when it is already installed. Missing models download from the whisper.cpp model repository on first use.

## NVIDIA acceleration

With an NVIDIA GPU and a CUDA Toolkit that supports your Visual Studio C++ toolset, you can build with CUDA enabled.

```powershell
npm run tauri build -- --no-bundle --features cuda
```

Run `src-tauri/target/release/whispertray.exe` with the CUDA runtime DLLs available on PATH. The standard Windows build uses the CPU.

## License

MIT - see `LICENSE`.
