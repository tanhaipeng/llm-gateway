use crate::dispatcher::Dispatcher;
use crate::logging::RequestLogger;
use crate::metrics::MetricsCollector;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response, Json},
};
use bytes::Bytes;
use uuid::Uuid;

// 请求大小限制 (10MB)
const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024;

/// 性能指标端点
pub async fn metrics_handler() -> Json<serde_json::Value> {
    let collector = MetricsCollector::new();
    let metrics = collector.collect_metrics(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    ).await;
    
    Json(serde_json::to_value(metrics).unwrap_or_default())
}

pub async fn proxy_handler(
    State(dispatcher): State<Dispatcher>,
    Path(provider): Path<String>,
    body: Bytes,
) -> Response {
    // 生成请求ID
    let request_id = Uuid::new_v4().to_string();
    tracing::info!("[{}] Forwarding request to provider: {}", request_id, provider);

    // 检查请求大小
    if body.len() > MAX_REQUEST_SIZE {
        tracing::error!("[{}] Request body too large: {} bytes", request_id, body.len());
        return (
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            format!("Request body too large: {} bytes (max: {} bytes)", body.len(), MAX_REQUEST_SIZE),
        )
            .into_response();
    }

    // 检查请求体是否为空
    if body.is_empty() {
        tracing::error!("[{}] Request body is empty", request_id);
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Request body is empty",
        )
            .into_response();
    }

    // 解析请求信息
    let (model, is_stream) = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(json) => {
            let model = json.get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            let is_stream = json.get("stream")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            (model, is_stream)
        }
        Err(e) => {
            tracing::error!("[{}] Failed to parse request body as JSON: {}", request_id, e);
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("Invalid request body: {}", e),
            )
                .into_response();
        }
    };

    tracing::debug!("[{}] Request is_stream: {}, model: {}", request_id, is_stream, model);

    match dispatcher.get_provider(&provider) {
        Some(provider_client) => {
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
                Ok(Ok(response)) => {
                    // 获取状态码和token信息
                    let status_code = response.status().as_u16();
                    
                    // 对于非流式请求，尝试提取token信息
                    let (prompt_tokens, completion_tokens) = if !is_stream {
                        // 这里我们简化处理，实际可能需要从响应中提取
                        (0u64, 0u64)
                    } else {
                        (0u64, 0u64)
                    };
                    
                    // 记录成功请求
                    let logger = RequestLogger::new();
                    let tracker = logger.start_request(request_id.clone());
                    tracker.complete(
                        provider.clone(),
                        model.clone(),
                        is_stream,
                        status_code,
                        prompt_tokens,
                        completion_tokens,
                    ).await;
                    
                    response.into_response()
                }
                Ok(Err(e)) => {
                    tracing::error!("[{}] Error forwarding request: {}", request_id, e);
                    
                    // 记录失败请求
                    let logger = RequestLogger::new();
                    let tracker = logger.start_request(request_id.clone());
                    tracker.complete_error(
                        provider.clone(),
                        model.clone(),
                        is_stream,
                        500,
                        e.to_string(),
                    ).await;
                    
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error: {}", e),
                    )
                        .into_response()
                }
                Err(_) => {
                    tracing::error!("[{}] Request timeout", request_id);
                    
                    // 记录超时请求
                    let logger = RequestLogger::new();
                    let tracker = logger.start_request(request_id.clone());
                    tracker.complete_error(
                        provider.clone(),
                        model.clone(),
                        is_stream,
                        504,
                        "Request timeout".to_string(),
                    ).await;
                    
                    (
                        axum::http::StatusCode::GATEWAY_TIMEOUT,
                        "Request timeout",
                    )
                        .into_response()
                }
            }
        }
        None => {
            tracing::error!("[{}] Provider not found: {}", request_id, provider);
            
            // 记录提供商未找到错误
            let logger = RequestLogger::new();
            let tracker = logger.start_request(request_id.clone());
            tracker.complete_error(
                provider.clone(),
                model.clone(),
                is_stream,
                404,
                format!("Provider not found: {}", provider),
            ).await;
            
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
