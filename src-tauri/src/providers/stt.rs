//! Speech-to-Text provider implementations

use crate::error::{AppError, Result};
use crate::modes::SttProvider as SttProviderType;
use async_trait::async_trait;
use std::path::PathBuf;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// STT provider trait
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Transcribe audio samples to text
    async fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String>;

    /// Get the provider name
    fn name(&self) -> &str;
}

/// Local whisper.cpp provider
pub struct WhisperCppProvider {
    model_path: PathBuf,
}

impl WhisperCppProvider {
    /// Create a new whisper.cpp provider
    pub fn new(model_path: PathBuf) -> Self {
        Self { model_path }
    }
}

#[async_trait]
impl SttProvider for WhisperCppProvider {
    async fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String> {
        let model_path = self.model_path.clone();
        let samples = samples.to_vec();
        let language = language.map(|s| s.to_string());

        let result = tokio::task::spawn_blocking(move || {
            let params = WhisperContextParameters::default();
            let ctx = WhisperContext::new_with_params(model_path.to_str().unwrap(), params)
                .map_err(|e| AppError::Transcription(format!("Failed to create context: {}", e)))?;
            run_inference(&ctx, &samples, language.as_deref())
        })
        .await
        .map_err(|e| AppError::Transcription(format!("Task failed: {}", e)))??;

        Ok(result)
    }

    fn name(&self) -> &str {
        "whisper.cpp"
    }
}

// ---------------------------------------------------------------------------
// Shared inference kernel
// ---------------------------------------------------------------------------

/// Run a full whisper inference pass on an already-loaded context.
/// This is called both by `WhisperCppProvider` and by the `WhisperCache` thread.
fn run_inference(ctx: &WhisperContext, samples: &[f32], language: Option<&str>) -> Result<String> {
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

// ---------------------------------------------------------------------------
// Persistent WhisperContext cache
// ---------------------------------------------------------------------------

/// Sent through the mpsc channel to the worker thread.
struct WhisperJob {
    samples: Vec<f32>,
    language: Option<String>,
    reply_tx: std::sync::mpsc::Sender<Result<String>>,
}

/// A long-lived whisper.cpp context that persists between recordings.
///
/// A dedicated OS thread owns the `WhisperContext` (which is `!Send`) and
/// processes one inference job at a time via a bounded mpsc channel.
/// Callers interact through the async [`WhisperCache::transcribe`] method.
pub struct WhisperCache {
    tx: std::sync::mpsc::SyncSender<WhisperJob>,
    /// Name of the model currently loaded.
    pub model_name: String,
}

impl WhisperCache {
    /// Spawn the background thread and load the model.
    ///
    /// Returns an error only if the channel or thread can't be created;
    /// model-load errors are logged inside the thread (the first `transcribe`
    /// call will return an error if the model failed to load).
    pub fn new(model_path: PathBuf, model_name: String) -> Result<Self> {
        // Capacity 1: the caller always waits for a reply before sending again,
        // so the queue never needs to be deeper.
        let (tx, rx) = std::sync::mpsc::sync_channel::<WhisperJob>(1);

        std::thread::spawn(move || {
            log::info!("WhisperCache: loading model from {:?}", model_path);
            let ctx_params = WhisperContextParameters::default();
            let ctx = match WhisperContext::new_with_params(
                model_path.to_str().unwrap_or(""),
                ctx_params,
            ) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("WhisperCache: failed to load model: {}", e);
                    return; // thread exits; senders will get a SendError next call
                }
            };
            log::info!("WhisperCache: model loaded, waiting for jobs");

            while let Ok(job) = rx.recv() {
                let result = run_inference(&ctx, &job.samples, job.language.as_deref());
                let _ = job.reply_tx.send(result);
            }
            log::info!("WhisperCache: shutting down");
        });

        Ok(Self { tx, model_name })
    }

    /// Submit an inference job and await the result.
    ///
    /// Sends the job to the background thread via a blocking channel, so this
    /// method offloads the blocking `recv` into `spawn_blocking`.
    pub async fn transcribe(&self, samples: Vec<f32>, language: Option<String>) -> Result<String> {
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            tx.send(WhisperJob {
                samples,
                language,
                reply_tx,
            })
            .map_err(|_| AppError::Transcription("Whisper worker disconnected".to_string()))?;

            reply_rx
                .recv()
                .map_err(|_| AppError::Transcription("Whisper worker did not reply".to_string()))?
        })
        .await
        .map_err(|e| AppError::Transcription(format!("Task join failed: {}", e)))?
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
            let provider = WhisperCppProvider::new(model_path);
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
