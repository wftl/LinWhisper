//! Mode management for WhisperTray
//!
//! Modes define how transcription and AI processing behave.
//! They are stored as JSON files in ~/.config/whispertray/modes/

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// STT provider options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SttProvider {
    WhisperCpp,
    WhisperServer,  // Self-hosted whisper server (Speaches, faster-whisper-server, etc.)
    OpenAI,         // Cloud OpenAI Whisper API
    Deepgram,
    Custom(String),
}

impl Default for SttProvider {
    fn default() -> Self {
        SttProvider::WhisperCpp
    }
}

/// LLM provider options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
    Ollama,
    Custom(String),
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::Ollama
    }
}

/// Output format options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Plain,
    Markdown,
}

/// A dictation mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mode {
    /// Unique identifier for the mode
    pub key: String,

    /// Display name
    pub name: String,

    /// Description of what this mode does
    pub description: String,

    /// STT provider to use
    #[serde(default)]
    pub stt_provider: SttProvider,

    /// STT model identifier (e.g., "large-v3", "base.en")
    #[serde(default = "default_stt_model")]
    pub stt_model: String,

    /// Whether to run AI processing after transcription
    #[serde(default)]
    pub ai_processing: bool,

    /// LLM provider to use (if ai_processing is true)
    #[serde(default)]
    pub llm_provider: LlmProvider,

    /// LLM model identifier
    #[serde(default)]
    pub llm_model: String,

    /// Prompt template for LLM processing
    /// Supports variables: {{transcript}}, {{context}}, {{language}}
    #[serde(default)]
    pub prompt_template: String,

    /// Output format
    #[serde(default)]
    pub output_format: OutputFormat,

    /// Whether this is a built-in mode
    #[serde(default)]
    pub builtin: bool,
}

fn default_stt_model() -> String {
    "base.en".to_string()
}

impl Default for Mode {
    fn default() -> Self {
        Mode {
            key: "voice_to_text".to_string(),
            name: "Voice to Text".to_string(),
            description: "Simple voice transcription without AI processing".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: false,
            llm_provider: LlmProvider::Ollama,
            llm_model: String::new(),
            prompt_template: String::new(),
            output_format: OutputFormat::Plain,
            builtin: true,
        }
    }
}

/// Get the modes directory path
pub fn get_modes_dir() -> Result<PathBuf> {
    let config_dir = directories::ProjectDirs::from("com", "whispertray", "WhisperTray")
        .ok_or_else(|| AppError::Config("Could not determine config directory".to_string()))?
        .config_dir()
        .to_path_buf();

    Ok(config_dir.join("modes"))
}

/// Create built-in modes
pub fn create_builtin_modes() -> Vec<Mode> {
    vec![
        Mode {
            key: "voice_to_text".to_string(),
            name: "Voice to Text".to_string(),
            description: "Simple voice transcription without AI processing".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: false,
            llm_provider: LlmProvider::Ollama,
            llm_model: String::new(),
            prompt_template: String::new(),
            output_format: OutputFormat::Plain,
            builtin: true,
        },
        Mode {
            key: "message".to_string(),
            name: "Message".to_string(),
            description: "Short casual message, cleaned up for chat/SMS".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: true,
            llm_provider: LlmProvider::Ollama,
            llm_model: "llama3.2".to_string(),
            prompt_template: r#"You are a helpful assistant that cleans up voice transcriptions into short, casual messages suitable for chat or SMS.

Instructions:
- Fix any transcription errors or unclear words
- Remove filler words (um, uh, like, you know)
- Keep the casual, conversational tone
- Keep it concise
- Do not add any preamble or explanation, just output the cleaned message

{{#if context}}
Context (for reference only):
{{context}}
{{/if}}

Transcript to clean up:
{{transcript}}

Cleaned message:"#.to_string(),
            output_format: OutputFormat::Plain,
            builtin: true,
        },
        Mode {
            key: "email".to_string(),
            name: "Email".to_string(),
            description: "Format transcription as a professional email with subject and body".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: true,
            llm_provider: LlmProvider::Ollama,
            llm_model: "llama3.2".to_string(),
            prompt_template: r#"You are a helpful assistant that converts voice transcriptions into professional emails.

Instructions:
- Create a clear, professional email from the spoken content
- Include a concise subject line
- Structure the body with proper greeting, content, and sign-off
- Fix any transcription errors
- Maintain a professional but friendly tone
- Format as:
  Subject: [subject]

  [body]

{{#if context}}
Context (for reference only):
{{context}}
{{/if}}

Transcript:
{{transcript}}

Email:"#.to_string(),
            output_format: OutputFormat::Plain,
            builtin: true,
        },
        Mode {
            key: "note".to_string(),
            name: "Note".to_string(),
            description: "Convert transcription into organized bullet points".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: true,
            llm_provider: LlmProvider::Ollama,
            llm_model: "llama3.2".to_string(),
            prompt_template: r#"You are a helpful assistant that converts voice transcriptions into organized notes.

Instructions:
- Extract key points from the transcription
- Organize into clear bullet points
- Group related items together
- Fix any transcription errors
- Be concise but capture all important information

{{#if context}}
Context (for reference only):
{{context}}
{{/if}}

Transcript:
{{transcript}}

Notes:"#.to_string(),
            output_format: OutputFormat::Markdown,
            builtin: true,
        },
        Mode {
            key: "meeting".to_string(),
            name: "Meeting".to_string(),
            description: "Create meeting summary with key points and action items".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: true,
            llm_provider: LlmProvider::Ollama,
            llm_model: "llama3.2".to_string(),
            prompt_template: r#"You are a helpful assistant that creates meeting summaries from transcriptions.

Instructions:
- Create a structured meeting summary
- Include:
  - Brief overview (2-3 sentences)
  - Key discussion points
  - Decisions made
  - Action items (with owners if mentioned)
- Fix any transcription errors
- Be concise but comprehensive

{{#if context}}
Context (for reference only):
{{context}}
{{/if}}

Transcript:
{{transcript}}

Meeting Summary:"#.to_string(),
            output_format: OutputFormat::Markdown,
            builtin: true,
        },
        Mode {
            key: "super".to_string(),
            name: "Super".to_string(),
            description: "Adaptive mode that intelligently formats based on content".to_string(),
            stt_provider: SttProvider::WhisperCpp,
            stt_model: "base.en".to_string(),
            ai_processing: true,
            llm_provider: LlmProvider::Ollama,
            llm_model: "llama3.2".to_string(),
            prompt_template: r#"You are a helpful assistant that intelligently processes voice transcriptions.

Instructions:
- Analyze the content and determine the best output format
- If it's a question, provide a helpful answer
- If it's a task or reminder, format it clearly
- If it's a message, clean it up appropriately
- If it's notes or ideas, organize them logically
- If it's code-related, format appropriately with any relevant syntax
- Fix any transcription errors
- Output only the processed result, no explanation

{{#if context}}
Context (for reference only):
{{context}}
{{/if}}

Transcript:
{{transcript}}

Output:"#.to_string(),
            output_format: OutputFormat::Plain,
            builtin: true,
        },
    ]
}

/// Load all modes from the modes directory and combine with built-ins
pub async fn load_modes() -> Result<HashMap<String, Mode>> {
    let mut modes = HashMap::new();

    // Add built-in modes first
    for mode in create_builtin_modes() {
        modes.insert(mode.key.clone(), mode);
    }

    // Load custom modes from config directory
    let modes_dir = get_modes_dir()?;

    if modes_dir.exists() {
        let mut entries = tokio::fs::read_dir(&modes_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                match load_mode_from_file(&path).await {
                    Ok(mode) => {
                        log::info!("Loaded custom mode: {}", mode.key);
                        modes.insert(mode.key.clone(), mode);
                    }
                    Err(e) => {
                        log::warn!("Failed to load mode from {:?}: {}", path, e);
                    }
                }
            }
        }
    } else {
        // Create modes directory and save built-in modes
        tokio::fs::create_dir_all(&modes_dir).await?;
        for mode in create_builtin_modes() {
            let path = modes_dir.join(format!("{}.json", mode.key));
            save_mode_to_file(&mode, &path).await?;
        }
    }

    Ok(modes)
}

/// Load a single mode from a JSON file
pub async fn load_mode_from_file(path: &PathBuf) -> Result<Mode> {
    let content = tokio::fs::read_to_string(path).await?;
    let mode: Mode = serde_json::from_str(&content)?;
    Ok(mode)
}

/// Save a mode to a JSON file
pub async fn save_mode_to_file(mode: &Mode, path: &PathBuf) -> Result<()> {
    let content = serde_json::to_string_pretty(mode)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

/// Save a mode (creates or updates)
pub async fn save_mode(mode: &Mode) -> Result<()> {
    let modes_dir = get_modes_dir()?;
    tokio::fs::create_dir_all(&modes_dir).await?;
    let path = modes_dir.join(format!("{}.json", mode.key));
    save_mode_to_file(mode, &path).await
}

/// Delete a custom mode
pub async fn delete_mode(key: &str) -> Result<()> {
    let modes_dir = get_modes_dir()?;
    let path = modes_dir.join(format!("{}.json", key));

    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }

    Ok(())
}

/// Render a prompt template with the given variables
pub fn render_prompt(template: &str, transcript: &str, context: Option<&str>, language: &str) -> String {
    let mut result = template.to_string();

    // Replace variables
    result = result.replace("{{transcript}}", transcript);
    result = result.replace("{{language}}", language);

    // Handle conditional context block
    if let Some(ctx) = context {
        result = result.replace("{{#if context}}", "");
        result = result.replace("{{/if}}", "");
        result = result.replace("{{context}}", ctx);
    } else {
        // Remove the entire context block if no context
        let re = regex::Regex::new(r"\{\{#if context\}\}[\s\S]*?\{\{/if\}\}").ok();
        if let Some(regex) = re {
            result = regex.replace_all(&result, "").to_string();
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_builtin_modes() {
        let modes = create_builtin_modes();
        assert!(!modes.is_empty());
        assert!(modes.iter().any(|m| m.key == "voice_to_text"));
        assert!(modes.iter().any(|m| m.key == "message"));
        assert!(modes.iter().any(|m| m.key == "email"));
    }

    #[test]
    fn test_mode_serialization() {
        let mode = Mode::default();
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: Mode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode.key, deserialized.key);
    }

    #[test]
    fn test_render_prompt_basic() {
        let template = "Transcript: {{transcript}}\nLanguage: {{language}}";
        let result = render_prompt(template, "Hello world", None, "en");
        assert!(result.contains("Hello world"));
        assert!(result.contains("en"));
    }

    #[test]
    fn test_render_prompt_with_context() {
        let template = "{{#if context}}Context: {{context}}{{/if}}\nTranscript: {{transcript}}";
        let result = render_prompt(template, "Hello", Some("Previous message"), "en");
        assert!(result.contains("Previous message"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_render_prompt_without_context() {
        let template = "{{#if context}}Context: {{context}}{{/if}}Transcript: {{transcript}}";
        let result = render_prompt(template, "Hello", None, "en");
        assert!(!result.contains("Context:"));
        assert!(result.contains("Hello"));
    }
}
