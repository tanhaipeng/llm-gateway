use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    OpenAI,
    Anthropic,
    #[serde(untagged)]
    Custom(String),
}

impl Provider {
    pub fn from_str(s: &str) -> Result<Self, crate::types::GatewayError> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Provider::OpenAI),
            "anthropic" => Ok(Provider::Anthropic),
            other => Ok(Provider::Custom(other.to_string())),
        }
    }

    pub fn is_openai_compatible(&self) -> bool {
        matches!(self, Provider::OpenAI | Provider::Custom(_))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub models: Vec<String>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub version: Option<String>,
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
