# WhisperTray

A local-first, tray-based dictation tool for Linux with optional AI post-processing.

![WhisperTray](./docs/screenshot.png)

## Features

- **System Tray Integration**: Quick access via tray icon with status indicators
  - 🟡 Yellow: Loading model
  - 🔴 Red: Recording in progress
  - 🔵 Blue: Processing
  - 🟢 Green: Ready
- **Local-First**: All transcription happens locally using whisper.cpp by default
- **Multiple Modes**: Built-in modes for different use cases (message, email, notes, meeting summaries)
- **AI Post-Processing**: Optional LLM processing to transform transcripts
- **History**: Full history with search, reprocessing, and export capabilities
- **Privacy-Focused**: Audio and transcripts stored locally; cloud providers only when enabled

## Requirements

### System Dependencies

```bash
# Ubuntu/Debian
sudo apt install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    libsecret-1-dev \
    libclang-dev \
    libasound2-dev \
    libxdo-dev \
    cmake \
    ffmpeg

# Fedora
sudo dnf install -y \
    webkit2gtk4.1-devel \
    gtk3-devel \
    libappindicator-gtk3-devel \
    librsvg2-devel \
    openssl-devel \
    libsecret-devel \
    clang-devel \
    cmake \
    ffmpeg

# Arch Linux
sudo pacman -S \
    webkit2gtk-4.1 \
    gtk3 \
    libappindicator-gtk3 \
    librsvg \
    openssl \
    libsecret \
    clang \
    cmake \
    ffmpeg
```

### Rust & Node.js

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node.js (via nvm recommended)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
nvm install 20
```

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/whispertray.git
cd whispertray

# Install dependencies
npm install

# Build and run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

The built AppImage will be in `src-tauri/target/release/bundle/appimage/`.

### Model Download

On first run, WhisperTray will automatically download the whisper model you've selected. Models are stored in:
```
~/.local/share/whispertray/WhisperTray/models/
```

Available models:
- `tiny.en` (~75MB) - Fastest, English only
- `base.en` (~150MB) - Recommended balance, English only
- `small.en` (~500MB) - Better accuracy
- `medium.en` (~1.5GB) - High accuracy
- `large-v3` (~3GB) - Best accuracy, multilingual

## Usage

### Quick Start

1. Launch WhisperTray - it will appear in your system tray
2. **Left-click** the tray icon to start/stop recording
3. **Right-click** for the full menu
4. Speak your text, then click again to stop
5. The transcription will be copied to clipboard and optionally pasted

### Modes

WhisperTray includes several built-in modes:

| Mode | Description | AI Processing |
|------|-------------|---------------|
| Voice to Text | Raw transcription | No |
| Message | Short casual message cleanup | Yes |
| Email | Format as email with subject | Yes |
| Note | Organize into bullet points | Yes |
| Meeting | Summary with action items | Yes |
| Super | Adaptive formatting | Yes |

### Custom Modes

Create custom modes by adding JSON files to `~/.config/whispertray/modes/`:

```json
{
  "key": "code_review",
  "name": "Code Review",
  "description": "Format as code review comments",
  "stt_provider": "whispercpp",
  "stt_model": "base.en",
  "ai_processing": true,
  "llm_provider": "ollama",
  "llm_model": "llama3.2",
  "prompt_template": "Format this as a code review comment:\n\n{{transcript}}",
  "output_format": "plain"
}
```

### Deep Links

WhisperTray registers the `whispertray://` URL scheme:

```bash
# Process text with a specific mode
xdg-open "whispertray://mode/email?text=Hello%20world"

# Command line equivalent
whispertray --mode email --text "Hello world"
```

## Configuration

### Settings Location

- Config: `~/.config/whispertray/WhisperTray/`
- Data: `~/.local/share/whispertray/WhisperTray/`
- Modes: `~/.config/whispertray/modes/`

### API Keys

API keys for cloud providers (OpenAI, Anthropic) are stored securely in your system keyring using libsecret.

### Environment Variables

```bash
# Override Ollama URL
export OLLAMA_HOST=http://localhost:11434

# Enable debug logging
export RUST_LOG=whispertray=debug
```

## Wayland vs X11

WhisperTray works on both X11 and Wayland, but with some differences:

### X11
- Full support for all features
- Direct paste simulation works
- Global hotkeys fully supported

### Wayland
- Clipboard operations work normally
- **Direct paste may not work** in all applications due to Wayland security model
- Text is always copied to clipboard - you can paste manually with Ctrl+V
- Global hotkeys require additional configuration (see below)

### Wayland Hotkey Setup

On Wayland, global hotkeys require compositor-level configuration. Example for GNOME:

```bash
# Using gsettings
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whispertray/']"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whispertray/ name 'WhisperTray Toggle'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whispertray/ command 'whispertray --toggle'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whispertray/ binding '<Super>space'
```

## Troubleshooting

### No audio input

1. Check PipeWire/PulseAudio is running: `pactl info`
2. Verify microphone access: `pactl list sources`
3. Try selecting a specific device in Settings

### Model download fails

1. Check internet connection
2. Manually download from HuggingFace:
   ```bash
   mkdir -p ~/.local/share/whispertray/WhisperTray/models
   wget -O ~/.local/share/whispertray/WhisperTray/models/ggml-base.en.bin \
     https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
   ```

### Tray icon not visible

1. Ensure you have a system tray (e.g., `gnome-shell-extension-appindicator`)
2. On GNOME, install AppIndicator extension

### AI processing not working

1. For Ollama: Ensure Ollama is running (`ollama serve`)
2. For cloud providers: Check API keys in Settings

## Development

### Project Structure

```
whispertray/
├── src/                    # React frontend
│   ├── components/         # UI components
│   ├── pages/              # Page components
│   ├── stores/             # Zustand stores
│   ├── lib/                # API wrappers
│   └── types/              # TypeScript types
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── audio.rs        # Audio recording
│   │   ├── commands.rs     # Tauri commands
│   │   ├── database.rs     # SQLite history
│   │   ├── modes.rs        # Mode management
│   │   ├── paste.rs        # Clipboard/paste
│   │   ├── providers/      # STT/LLM providers
│   │   ├── state.rs        # App state
│   │   └── tray.rs         # System tray
│   └── icons/              # Tray icons
└── package.json
```

### Running Tests

```bash
# Rust tests
cd src-tauri && cargo test

# Frontend tests
npm run test
```

### Building AppImage

```bash
npm run tauri build
# Output: src-tauri/target/release/bundle/appimage/whispertray_0.1.0_amd64.AppImage
```

## Privacy

WhisperTray is designed with privacy in mind:

- **Local by default**: Transcription uses whisper.cpp locally
- **No telemetry**: No data is sent anywhere unless you enable cloud providers
- **Secure key storage**: API keys stored in system keyring
- **Local history**: All history stored in local SQLite database
- **Audio files**: Stored locally, can be deleted individually or in bulk

## License

MIT License - see [LICENSE](LICENSE)

## Acknowledgments

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Local speech recognition
- [Tauri](https://tauri.app/) - Desktop application framework
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio I/O
