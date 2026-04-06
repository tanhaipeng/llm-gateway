use crate::dispatcher::Dispatcher;
use crate::logging;
use crate::metrics;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Json, Response},
};
use bytes::Bytes;
use std::sync::{atomic::Ordering, Arc};
use uuid::Uuid;

/// 共享状态类型：(Dispatcher, RequestLogger, 启动时间)
type AppState = (Dispatcher, logging::RequestLogger, Arc<std::time::Instant>);

/// 性能监控端点处理器
pub async fn metrics_handler(
    State((_, request_logger, start_time)): State<AppState>,
) -> Json<serde_json::Value> {
    let collector = metrics::MetricsCollector::new();
    let uptime_seconds = start_time.elapsed().as_secs();
    let metrics = collector
        .collect_metrics_with_logger(&request_logger, uptime_seconds)
        .await;
    Json(serde_json::to_value(metrics).unwrap_or_default())
}

pub async fn proxy_handler(
    State((dispatcher, request_logger, _start_time)): State<AppState>,
    Path(provider): Path<String>,
    body: Bytes,
) -> Response {
    let request_id = Uuid::new_v4().to_string();
    tracing::info!(request_id = %request_id, provider = %provider, "Forwarding request");

    if body.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "Request body is empty").into_response();
    }

    // 解析请求元数据
    let (model, is_stream) = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(json) => {
            let model = json
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            let is_stream = json.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
            (model, is_stream)
        }
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("Invalid request body: {}", e),
            )
                .into_response();
        }
    };

    tracing::debug!(request_id = %request_id, model = %model, is_stream = is_stream);

    let Some(provider_client) = dispatcher.get_provider(&provider) else {
        tracing::error!(request_id = %request_id, provider = %provider, "Provider not found");
        let tracker = request_logger.start_request(request_id.clone());
        tracker
            .complete_error(provider.clone(), model.clone(), is_stream, 404,
                format!("Provider not found: {}", provider))
            .await;
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("Provider not found: {}", provider),
        )
            .into_response();
    };

    let tracker = request_logger.start_request(request_id.clone());

    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_secs(930),
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
                .complete_error(provider.clone(), model.clone(), is_stream, 504,
                    "Request timeout".to_string())
                .await;
            (axum::http::StatusCode::GATEWAY_TIMEOUT, "Request timeout").into_response()
        }

        Ok(Err(e)) => {
            let msg = format!("Provider error: {}", e);
            tracing::error!(request_id = %request_id, error = %msg);
            tracker
                .complete_error(provider.clone(), model.clone(), is_stream, 502, msg.clone())
                .await;
            (axum::http::StatusCode::BAD_GATEWAY, msg).into_response()
        }

        Ok(Ok((response, stream_counter))) => {
            let status_code = response.status().as_u16();

            if !is_stream {
                // 非流式：读取完整响应体，提取 token
                let (parts, resp_body) = response.into_parts();
                match axum::body::to_bytes(resp_body, 10 * 1024 * 1024).await {
                    Ok(bytes) => {
                        let (prompt, completion) = extract_usage(&bytes);
                        tracker
                            .complete(provider.clone(), model.clone(), false, status_code, prompt, completion)
                            .await;
                        axum::http::Response::from_parts(parts, axum::body::Body::from(bytes))
                            .into_response()
                    }
                    Err(e) => {
                        // M-6: 响应体超过 10MB 或读取失败，返回 502 而不是空 200
                        let msg = format!("Failed to read response body: {}", e);
                        tracing::error!(request_id = %request_id, error = %msg);
                        tracker
                            .complete_error(provider.clone(), model.clone(), false, 502, msg.clone())
                            .await;
                        (axum::http::StatusCode::BAD_GATEWAY, msg).into_response()
                    }
                }
            } else {
                // 流式：立即记录请求（token=0），后台补充 token
                tracker
                    .complete(provider.clone(), model.clone(), true, status_code, 0, 0)
                    .await;

                if let Some(counter) = stream_counter {
                    let logger_bg = request_logger.clone();
                    tokio::spawn(async move {
                        let poll = std::time::Duration::from_millis(100);
                        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
                        loop {
                            tokio::time::sleep(poll).await;
                            // C-2: exit as soon as stream signals done or deadline exceeded
                            if counter.done.load(Ordering::Acquire) || std::time::Instant::now() >= deadline {
                                let p = counter.prompt.load(Ordering::Relaxed);
                                let c = counter.completion.load(Ordering::Relaxed);
                                logger_bg.add_tokens(p, c).await;
                                break;
                            }
                        }
                    });
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
            let p = u.get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| u.get("input_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let c = u.get("completion_tokens")
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
