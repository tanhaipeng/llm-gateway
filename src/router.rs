use crate::dispatcher::Dispatcher;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use bytes::Bytes;

// 请求大小限制 (10MB)
const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024;

pub async fn proxy_handler(
    State(dispatcher): State<Dispatcher>,
    Path(provider): Path<String>,
    body: Bytes,
) -> Response {
    tracing::info!("Forwarding request to provider: {}", provider);

    // 检查请求大小
    if body.len() > MAX_REQUEST_SIZE {
        tracing::error!("Request body too large: {} bytes", body.len());
        return (
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            format!("Request body too large: {} bytes (max: {} bytes)", body.len(), MAX_REQUEST_SIZE),
        )
            .into_response();
    }

    // 检查请求体是否为空
    if body.is_empty() {
        tracing::error!("Request body is empty");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Request body is empty",
        )
            .into_response();
    }

    match dispatcher.get_provider(&provider) {
        Some(provider_client) => {
            // 直接从请求体中判断是否为流式请求
            let is_stream = match serde_json::from_slice::<serde_json::Value>(&body) {
                Ok(json) => json.get("stream")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                Err(e) => {
                    tracing::error!("Failed to parse request body as JSON: {}", e);
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        format!("Invalid request body: {}", e),
                    )
                        .into_response();
                }
            };

            tracing::debug!("Request is_stream: {}", is_stream);

            // 使用超时保护
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(930), // 比客户端超时稍长
                async move {
                    if is_stream {
                        provider_client.forward_request_stream(body).await
                    } else {
                        provider_client.forward_request(body).await
                    }
                },
            ).await;

            match result {
                Ok(Ok(response)) => response.into_response(),
                Ok(Err(e)) => {
                    tracing::error!("Error forwarding request: {}", e);
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error: {}", e),
                    )
                        .into_response()
                }
                Err(_) => {
                    tracing::error!("Request timeout");
                    (
                        axum::http::StatusCode::GATEWAY_TIMEOUT,
                        "Request timeout",
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
