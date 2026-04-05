use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// 流式响应类型
pub type BoxTryStream<I> = Pin<Box<dyn Stream<Item = Result<I, crate::types::GatewayError>> + Send>>;
pub type SSEStream = BoxTryStream<Bytes>;

/// 流式响应错误
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Stream error: {0}")]
    StreamError(#[from] Box<reqwest_eventsource::Error>),

    #[error("Body error: {0}")]
    BodyError(String),

    #[error("Invalid chunk: {0}")]
    InvalidChunk(String),

    #[error("Stream ended unexpectedly")]
    StreamEnded,
}

impl StreamError {
    /// 检查错误是否可以重试
    pub fn is_retryable(&self) -> bool {
        match self {
            StreamError::StreamError(error) => match &**error {
                reqwest_eventsource::Error::Utf8(_)
                | reqwest_eventsource::Error::Parser(_)
                | reqwest_eventsource::Error::Transport(_) => true,
                reqwest_eventsource::Error::InvalidStatusCode(status_code, _) => {
                    status_code.is_server_error()
                }
                reqwest_eventsource::Error::InvalidLastEventId(_)
                | reqwest_eventsource::Error::InvalidContentType(_, _)
                | reqwest_eventsource::Error::StreamEnded => false,
            },
            StreamError::BodyError(_) | StreamError::InvalidChunk(_) | StreamError::StreamEnded => {
                false
            }
        }
    }
}

/// 流式响应头
pub fn stream_response_headers() -> reqwest::header::HeaderMap {
    reqwest::header::HeaderMap::from_iter([
        (
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("text/event-stream; charset=utf-8"),
        ),
        (
            reqwest::header::CONNECTION,
            reqwest::header::HeaderValue::from_static("keep-alive"),
        ),
        (
            reqwest::header::TRANSFER_ENCODING,
            reqwest::header::HeaderValue::from_static("chunked"),
        ),
        (
            reqwest::header::CACHE_CONTROL,
            reqwest::header::HeaderValue::from_static("no-cache"),
        ),
    ])
}
