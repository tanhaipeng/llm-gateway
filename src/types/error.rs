use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    // 配置无效
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    // 请求无效（客户端输入问题）
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    // 服务不可用（并发保护/熔断等）
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    // HTTP 网络错误（reqwest）
    #[error("HTTP request error: {0}")]
    HttpError(#[from] reqwest::Error),

    // IO 错误
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    // JSON 序列化/反序列化错误
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    // YAML 配置错误
    #[error("YAML error: {0}")]
    YamlError(#[from] serde_yml::Error),

    // 流式错误
    #[error("Stream error: {0}")]
    StreamError(#[from] crate::types::stream::StreamError),

    // Axum HTTP 构建错误
    #[error("Axum HTTP error: {0}")]
    AxumError(#[from] axum::http::Error),
}
