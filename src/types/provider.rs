use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    OpenAI,
    Anthropic,
    GoogleGemini,
    Deepseek,
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::OpenAI => write!(f, "openai"),
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::GoogleGemini => write!(f, "gemini"),
            Provider::Deepseek => write!(f, "deepseek"),
            Provider::Custom(name) => write!(f, "{}", name),
        }
    }
}

impl Provider {
    pub fn from_str(s: &str) -> Result<Self, crate::types::GatewayError> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Provider::OpenAI),
            "anthropic" => Ok(Provider::Anthropic),
            "gemini" | "google-gemini" | "google" => Ok(Provider::GoogleGemini),
            "deepseek" => Ok(Provider::Deepseek),
            other => Ok(Provider::Custom(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProviderConfig {
    pub models: Vec<String>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub providers: HashMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "0.0.0.0".to_string(),
            port: 8080,
        }
    }
}
