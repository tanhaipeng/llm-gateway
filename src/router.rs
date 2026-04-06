use crate::dispatcher::Dispatcher;
use crate::logging;
use crate::metrics;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::Bytes;
use std::sync::{atomic::Ordering, Arc};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub dispatcher: Dispatcher,
    pub request_logger: logging::RequestLogger,
    pub start_time: Arc<std::time::Instant>,
    pub request_timeout_seconds: u64,
}

/// 性能监控端点处理器
pub async fn metrics_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let collector = metrics::MetricsCollector::new();
    let uptime_seconds = state.start_time.elapsed().as_secs();
    let metrics = collector
        .collect_metrics_with_logger(&state.request_logger, uptime_seconds)
        .await;
    Json(serde_json::to_value(metrics).unwrap_or_default())
}

pub async fn proxy_handler(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    body: Bytes,
) -> Response {
    let dispatcher = &state.dispatcher;
    let request_logger = &state.request_logger;
    let request_id = Uuid::new_v4().to_string();
    tracing::info!(request_id = %request_id, provider = %provider, "Forwarding request");

    if body.is_empty() {
        let tracker = request_logger.start_request(request_id.clone());
        tracker
            .complete_error(
                provider.clone(),
                "unknown".to_string(),
                false,
                400,
                "Request body is empty".to_string(),
            )
            .await;
        return error_json_response(StatusCode::BAD_REQUEST, "Request body is empty");
    }

    // 解析请求元数据
    let (model, is_stream) = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(json) => {
            let model = json
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            let is_stream = json
                .get("stream")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            (model, is_stream)
        }
        Err(e) => {
            let tracker = request_logger.start_request(request_id.clone());
            tracker
                .complete_error(
                    provider.clone(),
                    "unknown".to_string(),
                    false,
                    400,
                    format!("Invalid request body: {}", e),
                )
                .await;
            return error_json_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {}", e),
            );
        }
    };

    tracing::debug!(request_id = %request_id, model = %model, is_stream = is_stream);

    let Some(provider_client) = dispatcher.get_provider(&provider) else {
        tracing::error!(request_id = %request_id, provider = %provider, "Provider not found");
        let tracker = request_logger.start_request(request_id.clone());
        tracker
            .complete_error(
                provider.clone(),
                model.clone(),
                is_stream,
                404,
                format!("Provider not found: {}", provider),
            )
            .await;
        return error_json_response(
            StatusCode::NOT_FOUND,
            &format!("Provider not found: {}", provider),
        );
    };

    let tracker = request_logger.start_request(request_id.clone());

    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_secs(state.request_timeout_seconds),
        async {
            if is_stream {
                provider_client
                    .forward_request_stream(body)
                    .await
                    .map(|(resp, counter)| (resp, Some(counter)))
            } else {
                provider_client
                    .forward_request(body)
                    .await
                    .map(|resp| (resp, None))
            }
        },
    )
    .await;

    match timeout_result {
        Err(_) => {
            tracing::error!(request_id = %request_id, "Request timed out");
            tracker
                .complete_error(
                    provider.clone(),
                    model.clone(),
                    is_stream,
                    504,
                    "Request timeout".to_string(),
                )
                .await;
            error_json_response(StatusCode::GATEWAY_TIMEOUT, "Request timeout")
        }

        Ok(Err(e)) => match e {
            crate::types::GatewayError::InvalidRequest(msg) => {
                tracing::warn!(request_id = %request_id, error = %msg, "Invalid mapped request");
                tracker
                    .complete_error(provider.clone(), model.clone(), is_stream, 400, msg.clone())
                    .await;
                error_json_response(StatusCode::BAD_REQUEST, &msg)
            }
            crate::types::GatewayError::ServiceUnavailable(msg) => {
                tracing::warn!(request_id = %request_id, error = %msg, "Service unavailable");
                tracker
                    .complete_error(provider.clone(), model.clone(), is_stream, 503, msg.clone())
                    .await;
                error_json_response(StatusCode::SERVICE_UNAVAILABLE, &msg)
            }
            other => {
                let msg = format!("Provider error: {}", other);
                tracing::error!(request_id = %request_id, error = %msg);
                tracker
                    .complete_error(provider.clone(), model.clone(), is_stream, 502, msg.clone())
                    .await;
                error_json_response(StatusCode::BAD_GATEWAY, &msg)
            }
        },

        Ok(Ok((response, stream_counter))) => {
            let status_code = response.status().as_u16();
            let is_http_success = (200..400).contains(&status_code);

            if !is_stream {
                // 非流式：读取完整响应体，提取 token
                let (parts, resp_body) = response.into_parts();
                match axum::body::to_bytes(resp_body, 10 * 1024 * 1024).await {
                    Ok(bytes) => {
                        let (prompt, completion) = extract_usage(&bytes);
                        if is_http_success {
                            tracker
                                .complete(
                                    provider.clone(),
                                    model.clone(),
                                    false,
                                    status_code,
                                    prompt,
                                    completion,
                                )
                                .await;
                        } else {
                            tracker
                                .complete_error(
                                    provider.clone(),
                                    model.clone(),
                                    false,
                                    status_code,
                                    format!("Provider returned status {}", status_code),
                                )
                                .await;
                        }
                        axum::http::Response::from_parts(parts, axum::body::Body::from(bytes))
                            .into_response()
                    }
                    Err(e) => {
                        // M-6: 响应体超过 10MB 或读取失败，返回 502 而不是空 200
                        let msg = format!("Failed to read response body: {}", e);
                        tracing::error!(request_id = %request_id, error = %msg);
                        tracker
                            .complete_error(
                                provider.clone(),
                                model.clone(),
                                false,
                                502,
                                msg.clone(),
                            )
                            .await;
                        error_json_response(StatusCode::BAD_GATEWAY, &msg)
                    }
                }
            } else {
                // 流式：HTTP 非成功直接记失败；HTTP 成功则等流结束后再记成功/失败
                if !is_http_success {
                    tracker
                        .complete_error(
                            provider.clone(),
                            model.clone(),
                            true,
                            status_code,
                            format!("Provider returned status {}", status_code),
                        )
                        .await;
                } else {
                    if let Some(counter) = stream_counter {
                        let tracker_bg = tracker;
                        let provider_bg = provider.clone();
                        let model_bg = model.clone();
                        let stream_wait_timeout_seconds = state.request_timeout_seconds;
                        tokio::spawn(async move {
                            if !counter.done.load(Ordering::Acquire) {
                                let wait_result = tokio::time::timeout(
                                    std::time::Duration::from_secs(stream_wait_timeout_seconds),
                                    counter.notify.notified(),
                                )
                                .await;
                                if wait_result.is_err() && !counter.done.load(Ordering::Acquire) {
                                    tracker_bg
                                        .complete_error(
                                            provider_bg,
                                            model_bg,
                                            true,
                                            504,
                                            "Stream completion timeout".to_string(),
                                        )
                                        .await;
                                    return;
                                }
                            }

                            if counter.errored.load(Ordering::Acquire) {
                                tracker_bg
                                    .complete_error(
                                        provider_bg,
                                        model_bg,
                                        true,
                                        502,
                                        "Stream terminated with error".to_string(),
                                    )
                                    .await;
                            } else {
                                let p = counter.prompt.load(Ordering::Relaxed);
                                let c = counter.completion.load(Ordering::Relaxed);
                                tracker_bg
                                    .complete(provider_bg, model_bg, true, status_code, p, c)
                                    .await;
                            }
                        });
                    } else {
                        tracker
                            .complete(provider.clone(), model.clone(), true, status_code, 0, 0)
                            .await;
                    }
                }

                response.into_response()
            }
        }
    }
}

/// 从响应体中提取 prompt/completion token 数
/// 支持 OpenAI 格式（prompt_tokens/completion_tokens）和
/// Anthropic 格式（input_tokens/output_tokens，在 convert_response 之前的原始响应）
fn extract_usage(bytes: &Bytes) -> (u64, u64) {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|j| {
            let u = j.get("usage")?;
            // OpenAI 格式（经过 ResponseMapper 转换后）
            let p = u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| u.get("input_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let c = u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| u.get("output_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            Some((p, c))
        })
        .unwrap_or((0, 0))
}

pub async fn health_handler() -> &'static str {
    "OK"
}

fn error_json_response(status: StatusCode, message: &str) -> Response {
    let error_type = if status.is_client_error() {
        "invalid_request_error"
    } else {
        "api_error"
    };
    (
        status,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": status.as_u16().to_string()
            }
        })),
    )
        .into_response()
}
