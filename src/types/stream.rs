use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// 流式响应类型
pub type BoxTryStream<I> =
    Pin<Box<dyn Stream<Item = Result<I, crate::types::GatewayError>> + Send>>;
pub type SSEStream = BoxTryStream<Bytes>;

/// 流式响应错误
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Stream body error: {0}")]
    BodyError(String),
}
