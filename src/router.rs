use crate::dispatcher::Dispatcher;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use bytes::Bytes;

pub async fn proxy_handler(
    State(dispatcher): State<Dispatcher>,
    Path(provider): Path<String>,
    body: Bytes,
) -> Response {
    tracing::info!("Forwarding request to provider: {}", provider);

    match dispatcher.get_provider(&provider) {
        Some(provider_client) => {
            // 检查是否是流式请求
            let is_stream = match provider_client.is_stream_request(&body) {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::error!("Failed to parse request body: {}", e);
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        format!("Invalid request body: {}", e),
                    )
                        .into_response();
                }
            };

            tracing::debug!("Request is_stream: {}", is_stream);

            // 根据是否流式选择不同的处理方式
            let result = if is_stream {
                provider_client.forward_request_stream(body).await
            } else {
                provider_client.forward_request(body).await
            };

            match result {
                Ok(response) => response.into_response(),
                Err(e) => {
                    tracing::error!("Error forwarding request: {}", e);
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error: {}", e),
                    )
                        .into_response()
                }
            }
        }
        None => {
            tracing::error!("Provider not found: {}", provider);
            (
                axum::http::StatusCode::NOT_FOUND,
                format!("Provider not found: {}", provider),
            )
                .into_response()
        }
    }
}

pub async fn health_handler() -> &'static str {
    "OK"
}
