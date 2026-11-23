//! Speech-to-Text provider implementations

use crate::error::{AppError, Result};
use crate::modes::SttProvider as SttProviderType;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::OnceCell;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// STT provider trait
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Transcribe audio samples to text
    async fn transcribe(&self, samples: Arc<Vec<f32>>, language: Option<&str>) -> Result<String>;

    /// Get the provider name
    fn name(&self) -> &str;
}

/// Local whisper.cpp provider
pub struct WhisperCppProvider {
    context: Arc<Mutex<WhisperContext>>,
}

impl WhisperCppProvider {
    /// Create a new whisper.cpp provider
    pub fn new(model_path: PathBuf) -> Result<Self> {
        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or_else(|| AppError::Transcription("Invalid model path".to_string()))?,
            params,
        )
        .map_err(|e| AppError::Transcription(format!("Failed to create context: {}", e)))?;

        Ok(Self {
            context: Arc::new(Mutex::new(ctx)),
        })
    }
}

#[async_trait]
impl SttProvider for WhisperCppProvider {
    async fn transcribe(&self, samples: Arc<Vec<f32>>, language: Option<&str>) -> Result<String> {
        let context = self.context.clone();
        let language = language.map(|s| s.to_string());

        let result = tokio::task::spawn_blocking(move || {
            let ctx = context.lock().map_err(|_| {
                AppError::Transcription("Failed to lock whisper context".to_string())
            })?;

            let mut state = ctx
                .create_state()
                .map_err(|e| AppError::Transcription(format!("Failed to create state: {}", e)))?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            // whisper.cpp defaults to four threads, even on CPUs with more cores.
            let threads = thread::available_parallelism().map_or(4, |count| count.get().min(8));
            params.set_n_threads(threads as i32);

            // Set language if specified
            if let Some(lang) = language.as_deref() {
                params.set_language(Some(lang));
            } else {
                params.set_language(Some("en"));
            }

            // Disable timestamps for cleaner output
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            // Run transcription
            state
                .full(params, &samples)
                .map_err(|e| AppError::Transcription(format!("Transcription failed: {}", e)))?;

            // Collect segments
            let num_segments = state
                .full_n_segments()
                .map_err(|e| AppError::Transcription(format!("Failed to get segments: {}", e)))?;

            let mut text = String::new();
            for i in 0..num_segments {
                if let Ok(segment) = state.full_get_segment_text(i) {
                    if !is_whisper_artifact(segment.trim()) {
                        text.push_str(&segment);
                    }
                }
            }

            Ok::<String, AppError>(text.trim().to_string())
        })
        .await
        .map_err(|e| AppError::Transcription(format!("Task failed: {}", e)))??;

        Ok(result)
    }

    fn name(&self) -> &str {
        "whisper.cpp"
    }
}

/// Get the default models directory
pub fn get_models_dir() -> Result<PathBuf> {
    let data_dir = directories::ProjectDirs::from("com", "whispertray", "WhisperTray")
        .ok_or_else(|| AppError::Config("Could not determine data directory".to_string()))?
        .data_dir()
        .to_path_buf();

    Ok(data_dir.join("models"))
}

/// Get the path to a specific model
pub fn get_model_path(model_name: &str) -> Result<PathBuf> {
    let models_dir = get_models_dir()?;
    Ok(models_dir.join(format!("ggml-{}.bin", model_name)))
}

/// Download a whisper model if not present
pub async fn ensure_model(model_name: &str) -> Result<PathBuf> {
    let model_path = get_model_path(model_name)?;

    if model_path.exists() {
        log::info!("Model already exists: {:?}", model_path);
        return Ok(model_path);
    }

    // Create models directory
    let models_dir = get_models_dir()?;
    tokio::fs::create_dir_all(&models_dir).await?;

    // Download model
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
        model_name
    );

    log::info!("Downloading model from: {}", url);

    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        return Err(AppError::Transcription(format!(
            "Failed to download model: HTTP {}",
            response.status()
        )));
    }

    let bytes = response.bytes().await?;
    tokio::fs::write(&model_path, &bytes).await?;

    log::info!("Model downloaded successfully: {:?}", model_path);
    Ok(model_path)
}

/// Create an STT provider based on configuration
pub async fn create_stt_provider(
    provider_type: &SttProviderType,
    model: &str,
) -> Result<Box<dyn SttProvider>> {
    match provider_type {
        SttProviderType::WhisperCpp => {
            let model_path = ensure_model(model).await?;
            let provider = tokio::task::spawn_blocking(move || WhisperCppProvider::new(model_path))
                .await
                .map_err(|e| {
                    AppError::Transcription(format!("Model loading task failed: {e}"))
                })??;
            Ok(Box::new(provider))
        }
        SttProviderType::Deepgram => Err(AppError::Provider(
            "Deepgram not yet implemented".to_string(),
        )),
        SttProviderType::OpenAI => Err(AppError::Provider(
            "OpenAI STT not yet implemented".to_string(),
        )),
        SttProviderType::Custom(name) => {
            Err(AppError::Provider(format!("Unknown provider: {}", name)))
        }
    }
}

type CachedProvider = Arc<OnceCell<Arc<dyn SttProvider>>>;

#[derive(Clone, Default)]
pub(crate) struct ProviderCache {
    models: Arc<Mutex<HashMap<(SttProviderType, String), CachedProvider>>>,
}

impl ProviderCache {
    pub(crate) async fn get(
        &self,
        provider_type: &SttProviderType,
        model: &str,
    ) -> Result<Arc<dyn SttProvider>> {
        let cell = self
            .models
            .lock()
            .map_err(|_| AppError::Provider("STT cache is unavailable".to_string()))?
            .entry((provider_type.clone(), model.to_string()))
            .or_default()
            .clone();

        let provider = cell
            .get_or_try_init(|| async {
                create_stt_provider(provider_type, model)
                    .await
                    .map(Arc::from)
            })
            .await?;
        Ok(provider.clone())
    }

    pub(crate) async fn warm_installed(&self, provider_type: &SttProviderType, model: &str) {
        if *provider_type == SttProviderType::WhisperCpp
            && get_model_path(model).is_ok_and(|path| path.is_file())
        {
            if let Err(error) = self.get(provider_type, model).await {
                log::warn!("Could not warm speech model {model}: {error}");
            }
        }
    }
}

/// Check if a whisper segment is a non-speech artifact marker rather than actual transcription.
/// Whisper.cpp emits these for silence, music, applause, and other non-speech audio.
fn is_whisper_artifact(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    // Whisper artifacts are wrapped in brackets or parentheses, e.g.
    // "[BLANK_AUDIO]", "[silence]", "(music)", "[MUSIC]", etc.
    let is_bracketed = (text.starts_with('[') && text.ends_with(']'))
        || (text.starts_with('(') && text.ends_with(')'));
    if !is_bracketed {
        return false;
    }
    let inner = &text[1..text.len() - 1];
    let lower = inner.to_lowercase();
    matches!(
        lower.as_str(),
        "blank_audio"
            | "blank audio"
            | "silence"
            | "music"
            | "applause"
            | "laughter"
            | "inaudible"
            | "no speech"
            | "no audio"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_path() {
        let path = get_model_path("base.en").unwrap();
        assert!(path.to_str().unwrap().contains("ggml-base.en.bin"));
    }

    #[test]
    fn test_whisper_artifacts_detected() {
        assert!(is_whisper_artifact("[BLANK_AUDIO]"));
        assert!(is_whisper_artifact("[blank_audio]"));
        assert!(is_whisper_artifact("[BLANK AUDIO]"));
        assert!(is_whisper_artifact("[silence]"));
        assert!(is_whisper_artifact("[Silence]"));
        assert!(is_whisper_artifact("(music)"));
        assert!(is_whisper_artifact("[MUSIC]"));
        assert!(is_whisper_artifact("[laughter]"));
        assert!(is_whisper_artifact("(applause)"));
        assert!(is_whisper_artifact("[inaudible]"));
        assert!(is_whisper_artifact(""));
    }

    #[test]
    fn test_real_speech_not_filtered() {
        assert!(!is_whisper_artifact("Hello world"));
        assert!(!is_whisper_artifact("I like music"));
        assert!(!is_whisper_artifact("The silence was deafening"));
        assert!(!is_whisper_artifact("[custom tag]")); // not a known artifact
    }
}
