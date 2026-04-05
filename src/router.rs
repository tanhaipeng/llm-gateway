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
            // 直接从请求体中判断是否为流式请求
            let is_stream = match serde_json::from_slice::<serde_json::Value>(&body) {
                Ok(json) => json.get("stream")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                Err(_) => {
                    tracing::error!("Failed to parse request body as JSON");
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        "Invalid request body: not valid JSON",
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
