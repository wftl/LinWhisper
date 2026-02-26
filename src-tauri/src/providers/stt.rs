//! Speech-to-Text provider implementations

use crate::error::{AppError, Result};
use crate::modes::SttProvider as SttProviderType;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::mpsc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// STT provider trait
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Transcribe audio samples to text
    async fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String>;

    /// Get the provider name
    fn name(&self) -> &str;
}

struct TranscriptionRequest {
    samples: Vec<f32>,
    language: Option<String>,
    response_tx: tokio::sync::oneshot::Sender<Result<String>>,
}

/// Local whisper.cpp provider.
///
/// Loads the model once on creation and keeps a dedicated worker thread alive.
/// All transcription requests are sent to the worker via a channel, so the
/// model weights are never reloaded between calls.
pub struct WhisperCppProvider {
    tx: mpsc::SyncSender<TranscriptionRequest>,
}

impl WhisperCppProvider {
    /// Create a new provider and load the model.  This is a blocking call and
    /// should be invoked from `tokio::task::spawn_blocking`.
    pub fn new(model_path: PathBuf) -> Result<Self> {
        let model_path_str = model_path
            .to_str()
            .ok_or_else(|| AppError::Transcription("Invalid model path".to_string()))?
            .to_string();

        let (tx, rx) = mpsc::sync_channel::<TranscriptionRequest>(1);
        // Channel used to propagate model-load success/failure back to the caller.
        let (init_tx, init_rx) = mpsc::channel::<Result<()>>();

        std::thread::spawn(move || {
            let params = WhisperContextParameters::default();
            let ctx = match WhisperContext::new_with_params(&model_path_str, params) {
                Ok(ctx) => {
                    let _ = init_tx.send(Ok(()));
                    ctx
                }
                Err(e) => {
                    let _ = init_tx.send(Err(AppError::Transcription(format!(
                        "Failed to load model: {}",
                        e
                    ))));
                    return;
                }
            };

            log::info!("WhisperCpp worker ready (model: {})", model_path_str);

            while let Ok(req) = rx.recv() {
                let result = do_transcribe(&ctx, &req.samples, req.language.as_deref());
                let _ = req.response_tx.send(result);
            }

            log::info!("WhisperCpp worker exiting");
        });

        // Block until the model is loaded (or fails).  Timeout after 120 s so
        // a corrupt model doesn't hang the app forever.
        init_rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .map_err(|_| AppError::Transcription("Model loading timed out".to_string()))??;

        Ok(Self { tx })
    }
}

fn do_transcribe(ctx: &WhisperContext, samples: &[f32], language: Option<&str>) -> Result<String> {
    let mut state = ctx
        .create_state()
        .map_err(|e| AppError::Transcription(format!("Failed to create state: {}", e)))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    if let Some(lang) = language {
        params.set_language(Some(lang));
    } else {
        params.set_language(Some("en"));
    }

    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, samples)
        .map_err(|e| AppError::Transcription(format!("Transcription failed: {}", e)))?;

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

    Ok(text.trim().to_string())
}

#[async_trait]
impl SttProvider for WhisperCppProvider {
    async fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let req = TranscriptionRequest {
            samples: samples.to_vec(),
            language: language.map(|s| s.to_string()),
            response_tx,
        };

        self.tx
            .send(req)
            .map_err(|_| AppError::Transcription("Transcription worker is unavailable".to_string()))?;

        response_rx
            .await
            .map_err(|_| AppError::Transcription("Transcription worker dropped response".to_string()))?
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

/// Create an STT provider based on configuration.
/// The provider caches the loaded model for the lifetime of the returned object.
pub async fn create_stt_provider(
    provider_type: &SttProviderType,
    model: &str,
) -> Result<Box<dyn SttProvider>> {
    match provider_type {
        SttProviderType::WhisperCpp => {
            let model_path = ensure_model(model).await?;
            // Loading the model is CPU-bound and blocking — run on the blocking thread pool.
            let provider = tokio::task::spawn_blocking(move || WhisperCppProvider::new(model_path))
                .await
                .map_err(|e| AppError::Transcription(format!("Task join error: {}", e)))??;
            Ok(Box::new(provider))
        }
        SttProviderType::Deepgram => {
            Err(AppError::Provider("Deepgram not yet implemented".to_string()))
        }
        SttProviderType::OpenAI => {
            Err(AppError::Provider("OpenAI STT not yet implemented".to_string()))
        }
        SttProviderType::Custom(name) => {
            Err(AppError::Provider(format!("Unknown provider: {}", name)))
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
