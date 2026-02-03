//! Model configuration types for agent LLM providers.

use serde::{Deserialize, Serialize};

/// LLM provider configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// OpenRouter API (supports multiple models).
    #[default]
    OpenRouter,
    /// Anthropic API (Claude models).
    Anthropic,
    /// OpenAI API (GPT models).
    OpenAI,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenRouter => write!(f, "openrouter"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
        }
    }
}

/// Model configuration for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier (e.g., "anthropic/claude-opus-4", "openai/gpt-4").
    pub model: String,

    /// Maximum tokens to generate in responses.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Temperature for response generation (0.0 to 2.0).
    /// Lower values are more deterministic, higher values more creative.
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// LLM provider to use.
    #[serde(default)]
    pub provider: Provider,

    /// Optional system prompt override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Optional API key override (if not using environment variable).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub api_key: Option<String>,
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_temperature() -> f32 {
    0.7
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model: "anthropic/claude-sonnet-4".into(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            provider: Provider::default(),
            system_prompt: None,
            api_key: None,
        }
    }
}

impl ModelConfig {
    /// Create a new model configuration with the given model ID.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// Create configuration for Claude Opus 4 via OpenRouter.
    pub fn claude_opus() -> Self {
        Self {
            model: "anthropic/claude-opus-4".into(),
            max_tokens: 8192,
            temperature: 0.7,
            provider: Provider::OpenRouter,
            system_prompt: None,
            api_key: None,
        }
    }

    /// Create configuration for Claude Sonnet 4 via OpenRouter.
    pub fn claude_sonnet() -> Self {
        Self {
            model: "anthropic/claude-sonnet-4".into(),
            max_tokens: 4096,
            temperature: 0.7,
            provider: Provider::OpenRouter,
            system_prompt: None,
            api_key: None,
        }
    }

    /// Set the maximum tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set the temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature.clamp(0.0, 2.0);
        self
    }

    /// Set the provider.
    pub fn with_provider(mut self, provider: Provider) -> Self {
        self.provider = provider;
        self
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_display() {
        assert_eq!(Provider::OpenRouter.to_string(), "openrouter");
        assert_eq!(Provider::Anthropic.to_string(), "anthropic");
        assert_eq!(Provider::OpenAI.to_string(), "openai");
    }

    #[test]
    fn test_model_config_default() {
        let config = ModelConfig::default();
        assert_eq!(config.model, "anthropic/claude-sonnet-4");
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.temperature, 0.7);
        assert_eq!(config.provider, Provider::OpenRouter);
    }

    #[test]
    fn test_model_config_builder() {
        let config = ModelConfig::new("test-model")
            .with_max_tokens(1000)
            .with_temperature(0.5)
            .with_provider(Provider::Anthropic)
            .with_system_prompt("You are helpful.");

        assert_eq!(config.model, "test-model");
        assert_eq!(config.max_tokens, 1000);
        assert_eq!(config.temperature, 0.5);
        assert_eq!(config.provider, Provider::Anthropic);
        assert_eq!(config.system_prompt, Some("You are helpful.".into()));
    }

    #[test]
    fn test_temperature_clamping() {
        let config = ModelConfig::default().with_temperature(5.0);
        assert_eq!(config.temperature, 2.0);

        let config = ModelConfig::default().with_temperature(-1.0);
        assert_eq!(config.temperature, 0.0);
    }

    #[test]
    fn test_serialization() {
        let config = ModelConfig::claude_opus();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ModelConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.model, parsed.model);
        assert_eq!(config.max_tokens, parsed.max_tokens);
        assert_eq!(config.provider, parsed.provider);
    }
}
