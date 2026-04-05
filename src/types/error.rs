use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    // HTTP 网络错误
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    // IO 错误
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    // JSON 序列化/反序列化错误
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    // YAML 配置错误
    #[error("YAML error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    // Provider 错误
    #[error("Provider error: {0}")]
    ProviderError(String),

    // 请求错误
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    // 超时错误
    #[error("Request timeout")]
    Timeout,

    // 流式错误
    #[error("Stream error: {0}")]
    StreamError(#[from] crate::types::stream::StreamError),

    // HTTP 错误
    #[error("HTTP error: {0}")]
    HttpError2(#[from] axum::http::Error),
}

pub type Result<T> = std::result::Result<T, GatewayError>;
