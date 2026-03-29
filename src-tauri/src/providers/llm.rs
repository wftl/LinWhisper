//! LLM provider implementations for AI post-processing

use crate::error::{AppError, Result};
use crate::modes::LlmProvider as LlmProviderType;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// LLM provider trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate a completion from the given prompt
    async fn complete(&self, prompt: &str) -> Result<String>;

    /// Get the provider name
    fn name(&self) -> &str;
}

/// Ollama provider for local LLM inference
pub struct OllamaProvider {
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(model: String, base_url: Option<String>) -> Self {
        Self {
            base_url: base_url
                .or_else(|| std::env::var("OLLAMA_HOST").ok())
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            model,
        }
    }
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let url = format!("{}/api/generate", self.base_url);

        let request = OllamaRequest {
            model: self.model.clone(),
            prompt: prompt.to_string(),
            stream: false,
        };

        let response = client
            .post(&url)
            .json(&request)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("Ollama request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "Ollama error ({}): {}",
                status, body
            )));
        }

        let result: OllamaResponse = response
            .json()
            .await
            .map_err(|e| AppError::Provider(format!("Failed to parse Ollama response: {}", e)))?;

        Ok(result.response.trim().to_string())
    }

    fn name(&self) -> &str {
        "Ollama"
    }
}

/// OpenAI provider
pub struct OpenAiProvider {
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessageResponse,
}

#[derive(Deserialize)]
struct OpenAiMessageResponse {
    content: String,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let url = "https://api.openai.com/v1/chat/completions";

        let request = OpenAiRequest {
            model: self.model.clone(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: 2048,
        };

        let response = client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("OpenAI request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "OpenAI error ({}): {}",
                status, body
            )));
        }

        let result: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| AppError::Provider(format!("Failed to parse OpenAI response: {}", e)))?;

        result
            .choices
            .first()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| AppError::Provider("No response from OpenAI".to_string()))
    }

    fn name(&self) -> &str {
        "OpenAI"
    }
}

/// Anthropic Claude provider
pub struct AnthropicProvider {
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let url = "https://api.anthropic.com/v1/messages";

        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 2048,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let response = client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("Anthropic request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "Anthropic error ({}): {}",
                status, body
            )));
        }

        let result: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| AppError::Provider(format!("Failed to parse Anthropic response: {}", e)))?;

        result
            .content
            .first()
            .map(|c| c.text.trim().to_string())
            .ok_or_else(|| AppError::Provider("No response from Anthropic".to_string()))
    }

    fn name(&self) -> &str {
        "Anthropic"
    }
}

/// Create an LLM provider based on configuration
pub fn create_llm_provider(
    provider_type: &LlmProviderType,
    model: &str,
    api_key: Option<&str>,
    server_url: Option<String>,
) -> Result<Box<dyn LlmProvider>> {
    match provider_type {
        LlmProviderType::Ollama => Ok(Box::new(OllamaProvider::new(model.to_string(), server_url))),
        LlmProviderType::OpenAI => {
            let key = api_key
                .ok_or_else(|| AppError::Provider("OpenAI API key required".to_string()))?;
            Ok(Box::new(OpenAiProvider::new(
                key.to_string(),
                model.to_string(),
            )))
        }
        LlmProviderType::Anthropic => {
            let key = api_key
                .ok_or_else(|| AppError::Provider("Anthropic API key required".to_string()))?;
            Ok(Box::new(AnthropicProvider::new(
                key.to_string(),
                model.to_string(),
            )))
        }
        LlmProviderType::Custom(name) => {
            Err(AppError::Provider(format!("Unknown LLM provider: {}", name)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_provider_creation() {
        let provider = OllamaProvider::new("llama3.2".to_string(), None);
        assert_eq!(provider.name(), "Ollama");
    }
}
