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
}
