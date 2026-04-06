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
    #[serde(default)]
    pub server: ServerConfig,
    pub providers: HashMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub request_timeout_seconds: u64,
    #[serde(default)]
    pub cors: CorsConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "0.0.0.0".to_string(),
            port: 8080,
            request_timeout_seconds: 930,
            cors: CorsConfig::default(),
            limits: LimitsConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CorsConfig {
    #[serde(default)]
    pub allow_any_origin: bool,
    #[serde(default = "default_cors_allow_origins")]
    pub allow_origins: Vec<String>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allow_any_origin: false,
            allow_origins: default_cors_allow_origins(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct LimitsConfig {
    #[serde(default)]
    pub max_in_flight_requests: Option<usize>,
    #[serde(default)]
    pub max_requests_per_second: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct MetricsConfig {
    #[serde(default)]
    pub require_auth: bool,
}

fn default_cors_allow_origins() -> Vec<String> {
    Vec::new()
}
