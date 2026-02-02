//! Application state management

use crate::audio::RecordingHandle;
use crate::database::{get_audio_dir, get_database_path, Database, HistoryItem};
use crate::error::{AppError, Result};
use crate::modes::{load_modes, Mode, LlmProvider as LlmProviderType, SttProvider as SttProviderType};
use crate::paste;
use crate::providers::{llm, stt};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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
    /// URL for self-hosted whisper server (used when stt_provider is WhisperServer)
    #[serde(default)]
    pub whisper_server_url: Option<String>,
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
            whisper_server_url: None,
        }
    }
}

/// Main application state (Send + Sync safe)
pub struct AppState {
    /// Tauri app handle
    pub app_handle: AppHandle,

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
}

impl AppState {
    /// Create new application state
    pub fn new(app_handle: AppHandle) -> Result<Self> {
        let settings = Self::load_settings()?;

        Ok(Self {
            app_handle,
            status: RecordingStatus::Loading,
            modes: HashMap::new(),
            active_mode_key: settings.active_mode_key.clone(),
            recording_handle: RecordingHandle::new(),
            database: None,
            settings,
            last_context: None,
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

        self.status = RecordingStatus::Ready;
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
        if self.is_recording() {
            return Err(AppError::RecordingInProgress);
        }

        // Capture context if enabled
        if self.settings.context_awareness {
            self.last_context = paste::get_clipboard_text().ok();
        }

        crate::audio::start_recording(
            self.recording_handle.clone(),
            &self.settings.input_device,
            level_callback,
        )?;
        self.status = RecordingStatus::Recording;

        Ok(())
    }

    /// Stop recording and process
    pub async fn stop_recording(&mut self) -> Result<String> {
        if !self.is_recording() {
            return Err(AppError::NoRecordingInProgress);
        }

        let samples = crate::audio::stop_recording(&self.recording_handle)?;
        self.status = RecordingStatus::Processing;

        // Helper to reset status on error
        let result = self.process_recording(samples).await;
        if result.is_err() {
            self.status = RecordingStatus::Ready;
        }
        result
    }

    /// Internal: process recorded samples (transcribe, AI, save history)
    async fn process_recording(&mut self, samples: Vec<f32>) -> Result<String> {
        // Get active mode
        let mode = self
            .get_active_mode()
            .cloned()
            .ok_or_else(|| AppError::ModeNotFound(self.active_mode_key.clone()))?;

        // Save audio file
        let audio_dir = get_audio_dir()?;
        tokio::fs::create_dir_all(&audio_dir).await?;

        let audio_id = Uuid::new_v4().to_string();
        let audio_path = audio_dir.join(format!("{}.wav", audio_id));
        crate::audio::save_wav(&samples, &audio_path)?;

        let duration_ms = crate::audio::calculate_duration_ms(samples.len());

        // Transcribe
        log::info!("Starting transcription...");
        let transcript = self.transcribe(&samples, &mode).await?;
        log::info!("Transcription complete: {} chars", transcript.len());

        // AI processing if enabled
        let output = if mode.ai_processing && !mode.prompt_template.is_empty() {
            log::info!("Starting AI processing...");
            match self.process_with_llm(&transcript, &mode).await {
                Ok(result) => result,
                Err(e) => {
                    log::warn!("AI processing failed: {}, using raw transcript", e);
                    transcript.clone()
                }
            }
        } else {
            transcript.clone()
        };

        // Save to history
        let history_item = HistoryItem {
            id: audio_id,
            created_at: Utc::now(),
            mode_key: mode.key.clone(),
            audio_path: Some(audio_path.to_string_lossy().to_string()),
            transcript_raw: transcript.clone(),
            output_final: output.clone(),
            stt_provider: format!("{:?}", mode.stt_provider).to_lowercase(),
            stt_model: mode.stt_model.clone(),
            llm_provider: if mode.ai_processing {
                Some(format!("{:?}", mode.llm_provider).to_lowercase())
            } else {
                None
            },
            llm_model: if mode.ai_processing {
                Some(mode.llm_model.clone())
            } else {
                None
            },
            duration_ms,
            error: None,
        };

        if let Some(db) = &self.database {
            let db = db.lock().unwrap();
            let _ = db.insert_history(&history_item);
        }

        // Copy to clipboard and paste
        let _ = paste::copy_and_paste(&output, self.settings.auto_paste);

        self.status = RecordingStatus::Ready;

        Ok(output)
    }

    /// Transcribe audio samples
    async fn transcribe(&self, samples: &[f32], mode: &Mode) -> Result<String> {
        let api_key = self.get_stt_api_key(&mode.stt_provider)?;
        let server_url = self.settings.whisper_server_url.clone();

        let provider = stt::create_stt_provider(
            &mode.stt_provider,
            &mode.stt_model,
            api_key,
            server_url,
        ).await?;

        provider
            .transcribe(samples, Some(&self.settings.language))
            .await
    }

    /// Process transcript with LLM
    async fn process_with_llm(&self, transcript: &str, mode: &Mode) -> Result<String> {
        // Get API key if needed
        let api_key = self.get_api_key(&mode.llm_provider)?;

        let provider = llm::create_llm_provider(
            &mode.llm_provider,
            &mode.llm_model,
            api_key.as_deref(),
        )?;

        let prompt = crate::modes::render_prompt(
            &mode.prompt_template,
            transcript,
            self.last_context.as_deref(),
            &self.settings.language,
        );

        provider.complete(&prompt).await
    }

    /// Get API key for an LLM provider from secure storage
    pub fn get_api_key(&self, provider: &LlmProviderType) -> Result<Option<String>> {
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

    /// Get API key for an STT provider from secure storage
    pub fn get_stt_api_key(&self, provider: &SttProviderType) -> Result<Option<String>> {
        let service = "whispertray";
        let key_name = match provider {
            SttProviderType::OpenAI => "openai_api_key", // Reuse same key as LLM
            SttProviderType::Deepgram => "deepgram_api_key",
            SttProviderType::WhisperCpp => return Ok(None),    // Local, no key needed
            SttProviderType::WhisperServer => return Ok(None), // Self-hosted, typically no auth
            SttProviderType::Custom(_) => return Ok(None),
        };

        match keyring::Entry::new(service, key_name) {
            Ok(entry) => match entry.get_password() {
                Ok(password) => Ok(Some(password)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AppError::Keyring(format!("Failed to get STT API key: {}", e))),
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
            Err(e) => Err(AppError::Keyring(format!("Failed to delete API key: {}", e))),
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
        self.recording_handle.set_recording(false);
        self.status = RecordingStatus::Ready;
    }
}

/// Shared state type for Tauri
pub type SharedState = Arc<tokio::sync::Mutex<AppState>>;
