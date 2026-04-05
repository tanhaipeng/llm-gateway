use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("HTTP client error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Timeout")]
    Timeout,

    #[error("Invalid header value: {0}")]
    InvalidHeader(String),

    #[error("HTTP error: {0}")]
    HttpError2(#[from] axum::http::Error),

    #[error("Invalid header value: {0}")]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),

    #[error("Stream error: {0}")]
    StreamError(#[from] crate::types::stream::StreamError),
}

pub type Result<T> = std::result::Result<T, GatewayError>;
