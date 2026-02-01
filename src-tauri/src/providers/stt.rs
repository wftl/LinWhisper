//! Speech-to-Text provider implementations

use crate::error::{AppError, Result};
use crate::modes::SttProvider as SttProviderType;
use async_trait::async_trait;
use reqwest::multipart;
use serde::Deserialize;
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
            // Create context for transcription
            let params = WhisperContextParameters::default();
            let ctx = WhisperContext::new_with_params(model_path.to_str().unwrap(), params)
                .map_err(|e| AppError::Transcription(format!("Failed to create context: {}", e)))?;

            let mut state = ctx
                .create_state()
                .map_err(|e| AppError::Transcription(format!("Failed to create state: {}", e)))?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

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
            let num_segments = state.full_n_segments().map_err(|e| {
                AppError::Transcription(format!("Failed to get segments: {}", e))
            })?;

            let mut text = String::new();
            for i in 0..num_segments {
                if let Ok(segment) = state.full_get_segment_text(i) {
                    text.push_str(&segment);
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

/// STT provider for self-hosted whisper servers
/// (e.g., Speaches, faster-whisper-server, LocalAI)
///
/// Uses the OpenAI-compatible /v1/audio/transcriptions API format.
pub struct SelfHostedWhisperSttProvider {
    base_url: String,
    model: String,
}

impl SelfHostedWhisperSttProvider {
    /// Create a new self-hosted whisper STT provider
    ///
    /// Reads base URL from WHISPER_API_URL env var.
    /// Example: WHISPER_API_URL=http://192.168.1.100:8000
    pub fn new(model: String) -> Self {
        let base_url = std::env::var("WHISPER_API_URL")
            .unwrap_or_else(|_| "http://localhost:8000".to_string());

        Self { base_url, model }
    }
}

/// Response format from OpenAI-compatible transcription API
#[derive(Deserialize)]
struct WhisperTranscriptionResponse {
    text: String,
}

#[async_trait]
impl SttProvider for SelfHostedWhisperSttProvider {
    async fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String> {
        // Convert f32 samples to WAV bytes
        let wav_data = samples_to_wav(samples)?;

        let client = reqwest::Client::new();
        let url = format!("{}/v1/audio/transcriptions", self.base_url);

        // Build multipart form
        let file_part = multipart::Part::bytes(wav_data)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| AppError::Transcription(format!("Failed to create multipart: {}", e)))?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone());

        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }

        let response = client
            .post(&url)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| AppError::Transcription(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Transcription(format!(
                "API error ({}): {}",
                status, body
            )));
        }

        let result: WhisperTranscriptionResponse = response
            .json()
            .await
            .map_err(|e| AppError::Transcription(format!("Failed to parse response: {}", e)))?;

        Ok(result.text.trim().to_string())
    }

    fn name(&self) -> &str {
        "Self-hosted Whisper"
    }
}

/// Convert f32 audio samples to WAV format bytes
fn samples_to_wav(samples: &[f32]) -> Result<Vec<u8>> {
    use std::io::Cursor;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| AppError::Transcription(format!("Failed to create WAV writer: {}", e)))?;

        for &sample in samples {
            let amplitude = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(amplitude)
                .map_err(|e| AppError::Transcription(format!("Failed to write sample: {}", e)))?;
        }

        writer.finalize()
            .map_err(|e| AppError::Transcription(format!("Failed to finalize WAV: {}", e)))?;
    }

    Ok(cursor.into_inner())
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
            // For self-hosted whisper servers (Speaches, faster-whisper-server, etc.)
            // Set WHISPER_API_URL env var to point to your server
            let provider = SelfHostedWhisperSttProvider::new(model.to_string());
            Ok(Box::new(provider))
        }
        SttProviderType::Custom(name) => {
            Err(AppError::Provider(format!("Unknown provider: {}", name)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_path() {
        let path = get_model_path("base.en").unwrap();
        assert!(path.to_str().unwrap().contains("ggml-base.en.bin"));
    }
}
