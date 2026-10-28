//! The summary model's provider settings, resolved from the database. Shared by the
//! summary service and the post-enhance title and tags step.

use crate::database::repositories::setting::SettingsRepository;
use crate::summary::llm_client::LLMProvider;
use sqlx::SqlitePool;
use tracing::info;

pub struct LlmConfig {
    pub provider: LLMProvider,
    pub model: String,
    pub api_key: String,
    pub ollama_endpoint: Option<String>,
    pub custom_openai_endpoint: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

/// Provider, key and endpoints for an explicit provider and model name.
pub async fn load_for(pool: &SqlitePool, provider: &str, model: &str) -> Result<LlmConfig, String> {
    let parsed = LLMProvider::from_str(provider)?;

    // Ollama, BuiltInAI and CustomOpenAI do not take a key from the standard column
    let api_key = if parsed == LLMProvider::Ollama || parsed == LLMProvider::BuiltInAI || parsed == LLMProvider::CustomOpenAI {
        String::new()
    } else {
        match SettingsRepository::get_api_key(pool, provider).await {
            Ok(Some(key)) if !key.is_empty() => key,
            Ok(None) | Ok(Some(_)) => return Err(format!("API key not found for {}", provider)),
            Err(e) => return Err(format!("Failed to retrieve API key for {}: {}", provider, e)),
        }
    };

    let ollama_endpoint = if parsed == LLMProvider::Ollama {
        match SettingsRepository::get_model_config(pool).await {
            Ok(Some(config)) => config.ollama_endpoint,
            Ok(None) => None,
            Err(e) => {
                info!("Failed to retrieve Ollama endpoint: {}, using default", e);
                None
            }
        }
    } else {
        None
    };

    let mut cfg = LlmConfig {
        provider: parsed.clone(),
        model: model.to_string(),
        api_key,
        ollama_endpoint,
        custom_openai_endpoint: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
    };

    if parsed == LLMProvider::CustomOpenAI {
        match SettingsRepository::get_custom_openai_config(pool).await {
            Ok(Some(config)) => {
                info!("✓ Using custom OpenAI endpoint: {}", config.endpoint);
                cfg.custom_openai_endpoint = Some(config.endpoint);
                // its own key (if any) replaces the empty string
                cfg.api_key = config.api_key.unwrap_or_default();
                cfg.max_tokens = config.max_tokens.map(|t| t as u32);
                cfg.temperature = config.temperature;
                cfg.top_p = config.top_p;
            }
            Ok(None) => return Err("Custom OpenAI provider selected but no configuration found".to_string()),
            Err(e) => return Err(format!("Failed to retrieve custom OpenAI config: {}", e)),
        }
    }
    Ok(cfg)
}

pub const NO_SUMMARY_MODEL: &str = "No summary model configured. Set one in Settings, Recordings, Summary model.";

/// The summary model configured in Settings.
pub async fn load(pool: &SqlitePool) -> Result<LlmConfig, String> {
    let row = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|e| format!("Failed to read the summary model setting: {}", e))?;
    match row {
        Some(s) if !s.provider.trim().is_empty() && !s.model.trim().is_empty() => load_for(pool, &s.provider, &s.model).await,
        _ => Err(NO_SUMMARY_MODEL.to_string()),
    }
}
