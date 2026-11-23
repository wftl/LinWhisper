//! Application state management

use crate::audio::RecordingHandle;
use crate::database::{get_audio_dir, get_database_path, Database, HistoryItem};
use crate::error::{AppError, Result};
use crate::indicator::{emit_processing, hide_indicator};
use crate::modes::{create_builtin_modes, load_modes, LlmProvider as LlmProviderType, Mode};
use crate::paste;
use crate::providers::{llm, stt};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::AppHandle;
use uuid::Uuid;

/// Recording status for the tray icon
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingStatus {
    /// Model is loading (yellow)
    Loading,
    /// Recording in progress (red)
    Recording,
    /// Processing transcription/LLM (blue)
    Processing,
    /// Idle/ready (green)
    Ready,
    /// Error state
    Error,
}

impl RecordingStatus {
    pub fn icon_name(&self) -> &'static str {
        match self {
            RecordingStatus::Loading => "tray-yellow",
            RecordingStatus::Recording => "tray-red",
            RecordingStatus::Processing => "tray-blue",
            RecordingStatus::Ready => "tray-green",
            RecordingStatus::Error => "tray-red",
        }
    }
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub default_stt_provider: String,
    pub default_stt_model: String,
    pub default_llm_provider: String,
    pub default_llm_model: String,
    pub active_mode_key: String,
    pub input_device: String,
    pub auto_paste: bool,
    pub context_awareness: bool,
    pub language: String,
    pub first_run_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_stt_provider: "whispercpp".to_string(),
            default_stt_model: "base.en".to_string(),
            default_llm_provider: "ollama".to_string(),
            default_llm_model: "llama3.2".to_string(),
            active_mode_key: "voice_to_text".to_string(),
            input_device: String::new(), // Empty means default
            auto_paste: true,
            context_awareness: false,
            language: "en".to_string(),
            first_run_completed: false,
        }
    }
}

/// Main application state (Send + Sync safe)
pub struct AppState {
    /// Current recording status
    pub status: RecordingStatus,

    /// Available modes
    pub modes: HashMap<String, Mode>,

    /// Active mode key
    pub active_mode_key: String,

    /// Recording handle (Send + Sync safe)
    pub recording_handle: RecordingHandle,

    /// Database connection (wrapped in Mutex for thread safety)
    pub database: Option<Arc<Mutex<Database>>>,

    /// Application settings
    pub settings: Settings,

    /// Last context (clipboard text)
    pub last_context: Option<String>,

    /// Cached STT providers keyed by provider/model.
    pub(crate) stt_provider_cache: stt::ProviderCache,
}

impl AppState {
    /// Create new application state
    pub fn new() -> Result<Self> {
        let settings = Self::load_settings()?;
        let builtin_modes = create_builtin_modes();
        let mut modes = HashMap::new();
        for mode in builtin_modes {
            modes.insert(mode.key.clone(), mode);
        }
        let active_mode_key = if modes.contains_key(&settings.active_mode_key) {
            settings.active_mode_key.clone()
        } else {
            "voice_to_text".to_string()
        };

        Ok(Self {
            status: RecordingStatus::Ready,
            modes,
            active_mode_key,
            recording_handle: RecordingHandle::new(),
            database: None,
            settings,
            last_context: None,
            stt_provider_cache: stt::ProviderCache::default(),
        })
    }

    /// Load settings from disk
    fn load_settings() -> Result<Settings> {
        let settings_path = Self::get_settings_path()?;

        if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path)?;
            let settings: Settings = serde_json::from_str(&content)?;
            Ok(settings)
        } else {
            Ok(Settings::default())
        }
    }

    /// Save settings to disk
    pub fn save_settings(&self) -> Result<()> {
        let settings_path = Self::get_settings_path()?;

        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&self.settings)?;
        std::fs::write(settings_path, content)?;

        Ok(())
    }

    /// Get settings file path
    fn get_settings_path() -> Result<PathBuf> {
        let config_dir = directories::ProjectDirs::from("com", "whispertray", "WhisperTray")
            .ok_or_else(|| AppError::Config("Could not determine config directory".to_string()))?
            .config_dir()
            .to_path_buf();

        Ok(config_dir.join("settings.json"))
    }

    /// Load modes from configuration
    pub async fn load_modes(&mut self) -> Result<()> {
        self.modes = load_modes().await?;
        log::info!("Loaded {} modes", self.modes.len());

        // Ensure active mode exists
        if !self.modes.contains_key(&self.active_mode_key) {
            self.active_mode_key = "voice_to_text".to_string();
        }

        Ok(())
    }

    /// Initialize database
    pub async fn init_database(&mut self) -> Result<()> {
        let db_path = get_database_path()?;
        let db = Database::new(&db_path)?;
        self.database = Some(Arc::new(Mutex::new(db)));
        log::info!("Database initialized at {:?}", db_path);
        Ok(())
    }

    /// Get the active mode
    pub fn get_active_mode(&self) -> Option<&Mode> {
        self.modes.get(&self.active_mode_key)
    }

    /// Set the active mode
    pub fn set_active_mode(&mut self, key: &str) -> Result<()> {
        if !self.modes.contains_key(key) {
            return Err(AppError::ModeNotFound(key.to_string()));
        }
        self.active_mode_key = key.to_string();
        self.settings.active_mode_key = key.to_string();
        self.save_settings()?;
        Ok(())
    }

    /// Check if recording is in progress
    pub fn is_recording(&self) -> bool {
        self.recording_handle.is_recording()
    }

    /// Start recording
    pub fn start_recording(&mut self) -> Result<()> {
        self.start_recording_with_callback(None)
    }

    /// Start recording with an optional level callback
    pub fn start_recording_with_callback(
        &mut self,
        level_callback: Option<crate::audio::LevelCallback>,
    ) -> Result<()> {
        if self.is_recording() || self.status == RecordingStatus::Processing {
            return Err(AppError::RecordingInProgress);
        }

        self.recording_handle =
            crate::audio::start_recording(&self.settings.input_device, level_callback)?;
        self.status = RecordingStatus::Recording;

        self.last_context = self
            .settings
            .context_awareness
            .then(paste::get_clipboard_text)
            .and_then(std::result::Result::ok);

        if let Some(mode) = self.get_active_mode().cloned() {
            let cache = self.stt_provider_cache.clone();
            tauri::async_runtime::spawn(async move {
                cache
                    .warm_installed(&mode.stt_provider, &mode.stt_model)
                    .await;
            });
        }

        Ok(())
    }

    /// Get API key for a provider from secure storage
    pub fn get_api_key(provider: &LlmProviderType) -> Result<Option<String>> {
        let service = "whispertray";
        let key_name = match provider {
            LlmProviderType::OpenAI => "openai_api_key",
            LlmProviderType::Anthropic => "anthropic_api_key",
            LlmProviderType::Ollama => return Ok(None), // Ollama doesn't need a key
            LlmProviderType::Custom(_) => return Ok(None),
        };

        match keyring::Entry::new(service, key_name) {
            Ok(entry) => match entry.get_password() {
                Ok(password) => Ok(Some(password)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AppError::Keyring(format!("Failed to get API key: {}", e))),
            },
            Err(e) => Err(AppError::Keyring(format!(
                "Failed to access keyring: {}",
                e
            ))),
        }
    }

    /// Save an API key to secure storage
    pub fn save_api_key(&self, provider: &str, key: &str) -> Result<()> {
        let service = "whispertray";
        let key_name = format!("{}_api_key", provider.to_lowercase());

        let entry = keyring::Entry::new(service, &key_name)
            .map_err(|e| AppError::Keyring(format!("Failed to access keyring: {}", e)))?;

        entry
            .set_password(key)
            .map_err(|e| AppError::Keyring(format!("Failed to save API key: {}", e)))?;

        Ok(())
    }

    /// Delete an API key from secure storage
    pub fn delete_api_key(&self, provider: &str) -> Result<()> {
        let service = "whispertray";
        let key_name = format!("{}_api_key", provider.to_lowercase());

        let entry = keyring::Entry::new(service, &key_name)
            .map_err(|e| AppError::Keyring(format!("Failed to access keyring: {}", e)))?;

        match entry.delete_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(AppError::Keyring(format!(
                "Failed to delete API key: {}",
                e
            ))),
        }
    }

    /// Check if an API key exists
    pub fn has_api_key(&self, provider: &str) -> bool {
        let service = "whispertray";
        let key_name = format!("{}_api_key", provider.to_lowercase());

        keyring::Entry::new(service, &key_name)
            .and_then(|entry| entry.get_password())
            .is_ok()
    }

    /// Cancel current recording
    pub fn cancel_recording(&mut self) {
        if crate::audio::stop_recording(&self.recording_handle).is_ok() {
            self.status = RecordingStatus::Ready;
        }
    }
}

/// Shared state type for Tauri
pub struct SharedState(pub tokio::sync::Mutex<AppState>);

/// Stop capture and hand off the recording without holding the app lock during inference.
pub async fn complete_recording(shared: &SharedState, handle: &AppHandle) -> Result<String> {
    let stopped_at = Instant::now();
    let (samples, mode, language, last_context, auto_paste, database, cache) = {
        let mut state = shared.0.lock().await;
        if !state.is_recording() {
            return Err(AppError::NoRecordingInProgress);
        }

        let mode = state
            .get_active_mode()
            .cloned()
            .ok_or_else(|| AppError::ModeNotFound(state.active_mode_key.clone()))?;
        let samples = crate::audio::stop_recording(&state.recording_handle)?;
        state.status = RecordingStatus::Processing;
        emit_processing(handle, true);
        let _ = crate::tray::update_tray_icon(handle, RecordingStatus::Processing);

        (
            Arc::new(samples),
            mode,
            state.settings.language.clone(),
            state.last_context.take(),
            state.settings.auto_paste,
            state.database.clone(),
            state.stt_provider_cache.clone(),
        )
    };
    let capture_ms = stopped_at.elapsed().as_millis();

    let result = async {
        let started = Instant::now();
        let provider = cache.get(&mode.stt_provider, &mode.stt_model).await?;
        let model_ms = started.elapsed().as_millis();

        let started = Instant::now();
        let transcript = provider.transcribe(samples.clone(), Some(&language)).await?;
        let speech_ms = started.elapsed().as_millis();

        let started = Instant::now();
        let output = if mode.ai_processing && !mode.prompt_template.is_empty() {
            let api_key = AppState::get_api_key(&mode.llm_provider)?;
            let provider =
                llm::create_llm_provider(&mode.llm_provider, &mode.llm_model, api_key.as_deref())?;
            let prompt = crate::modes::render_prompt(
                &mode.prompt_template,
                &transcript,
                last_context.as_deref(),
                &language,
            );

            match provider.complete(&prompt).await {
                Ok(result) => result,
                Err(e) => {
                    log::warn!("AI processing failed: {}, using raw transcript", e);
                    transcript.clone()
                }
            }
        } else {
            transcript.clone()
        };
        let ai_ms = started.elapsed().as_millis();

        let history_item = HistoryItem {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            mode_key: mode.key,
            audio_path: None,
            transcript_raw: transcript,
            output_final: output.clone(),
            stt_provider: format!("{:?}", mode.stt_provider).to_lowercase(),
            stt_model: mode.stt_model,
            llm_provider: mode
                .ai_processing
                .then(|| format!("{:?}", mode.llm_provider).to_lowercase()),
            llm_model: mode.ai_processing.then_some(mode.llm_model),
            duration_ms: crate::audio::calculate_duration_ms(samples.len()),
            error: None,
        };

        tokio::task::spawn_blocking(move || {
            if !output.is_empty() {
                let started = Instant::now();
                if let Err(error) = paste::copy_and_paste(&output, auto_paste) {
                    log::warn!("Could not deliver dictation: {error}");
                }
                log::info!(
                    "Dictation ({} ms audio): stop={} ms, model={} ms, speech={} ms, AI={} ms, clipboard/paste={} ms, total={} ms",
                    history_item.duration_ms,
                    capture_ms,
                    model_ms,
                    speech_ms,
                    ai_ms,
                    started.elapsed().as_millis(),
                    stopped_at.elapsed().as_millis(),
                );
            }

            if let Some(db) = database {
                if let Err(error) = save_recording_history(&db, &samples, history_item) {
                    log::warn!("Could not save dictation history: {error}");
                }
            }
            output
        })
        .await
        .map_err(|error| AppError::Audio(format!("Dictation delivery task failed: {error}")))
    }
    .await;

    let mut state = shared.0.lock().await;
    state.status = if result.is_ok() {
        RecordingStatus::Ready
    } else {
        RecordingStatus::Error
    };
    let _ = hide_indicator(handle);
    emit_processing(handle, false);
    let _ = crate::tray::update_tray_icon(handle, state.status);
    let _ = crate::tray::update_tray_menu(handle, &state).await;

    result
}

fn save_recording_history(
    db: &Mutex<Database>,
    samples: &[f32],
    mut item: HistoryItem,
) -> Result<()> {
    let audio_result = (|| {
        let dir = get_audio_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.wav", item.id));
        if let Err(error) = crate::audio::save_wav(samples, &path) {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        Ok(path)
    })();

    match &audio_result {
        Ok(path) => item.audio_path = Some(path.to_string_lossy().into_owned()),
        Err(error) => log::warn!("Could not save dictation audio: {error}"),
    }

    let insert = db
        .lock()
        .map_err(|_| AppError::Config("History database is unavailable".to_string()))
        .and_then(|db| db.insert_history(&item));
    if insert.is_err() {
        if let Ok(path) = audio_result {
            let _ = std::fs::remove_file(path);
        }
    }
    insert
}
