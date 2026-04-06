use crate::dispatcher::Dispatcher;
use crate::logging;
use crate::metrics;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response, Json},
};
use bytes::Bytes;
use uuid::Uuid;

// 请求大小限制 (10MB)
const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024;

/// 性能监控端点处理器
pub async fn metrics_handler(
    State((_, request_logger)): State<(Dispatcher, logging::RequestLogger)>,
) -> Json<serde_json::Value> {
    let collector = metrics::MetricsCollector::new();
    // 使用全局共享的 request_logger
    let metrics = collector.collect_metrics_with_logger(
        &request_logger,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    ).await;
    
    Json(serde_json::to_value(metrics).unwrap_or_default())
}

pub async fn proxy_handler(
    State((dispatcher, request_logger)): State<(Dispatcher, logging::RequestLogger)>,
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
            // 在调用provider之前创建tracker
            let tracker = request_logger.start_request(request_id.clone());
            
            // 使用超时保护
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(930), // 比客户端超时稍长
                async move {
                    if is_stream {
                        provider_client.forward_request_stream(body).await
                            .map(|(resp, counter)| (resp, Some(counter)))
                    } else {
                        provider_client.forward_request(body).await
                            .map(|resp| (resp, None))
                    }
                },
            ).await;

            match result {
                Ok(inner_result) => {
                    // 处理内部的 Result
                    match inner_result {
                        Ok((response, stream_token_counter)) => {
                            // 处理成功的响应
                            let status_code = response.status().as_u16();

                            // 尝试从响应中提取token信息
                            let (prompt_tokens, completion_tokens, final_response) = if !is_stream {
                                // 对于非流式响应，尝试提取usage信息
                                let (parts, body) = response.into_parts();

                                match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
                                    Ok(bytes) => {
                                        let (prompt, completion) = serde_json::from_slice::<serde_json::Value>(&bytes)
                                            .ok()
                                            .and_then(|json| {
                                                let prompt = json.get("usage")
                                                    .and_then(|u| u.get("prompt_tokens"))
                                                    .and_then(|t| t.as_u64())
                                                    .unwrap_or(0);
                                                let completion = json.get("usage")
                                                    .and_then(|u| u.get("completion_tokens"))
                                                    .and_then(|t| t.as_u64())
                                                    .unwrap_or(0);
                                                Some((prompt, completion))
                                            })
                                            .unwrap_or((0u64, 0u64));

                                        // 重新构建响应
                                        let new_response = axum::http::Response::from_parts(parts, axum::body::Body::from(bytes));
                                        (prompt, completion, new_response)
                                    }
                                    Err(_) => {
                                        let new_response = axum::http::Response::from_parts(parts, axum::body::Body::empty());
                                        (0u64, 0u64, new_response)
                                    }
                                }
                            } else {
                                // 流式响应：立即记录请求（token 先为 0），后台异步补充 token 数
                                let token_counter = stream_token_counter
                                    .expect("stream response should have token_counter");

                                // 立即记录请求，确保 /metrics 能实时看到请求数
                                tracker.complete(
                                    provider.clone(),
                                    model.clone(),
                                    true,
                                    status_code,
                                    0,
                                    0,
                                ).await;

                                // 后台任务：等流消费完毕后补记 token
                                let logger_bg = request_logger.clone();
                                tokio::spawn(async move {
                                    // 轮询等待 token 计数被写入（最多等待 15 分钟）
                                    let mut waited_ms = 0u64;
                                    let poll_interval_ms = 200u64;
                                    let max_wait_ms = 15 * 60 * 1000u64;
                                    loop {
                                        tokio::time::sleep(
                                            std::time::Duration::from_millis(poll_interval_ms)
                                        ).await;
                                        waited_ms += poll_interval_ms;

                                        let (prompt, completion) = *token_counter.lock().await;
                                        if prompt > 0 || completion > 0 || waited_ms >= max_wait_ms {
                                            logger_bg.add_tokens(prompt, completion).await;
                                            break;
                                        }
                                    }
                                });

                                return response.into_response();
                            };

                            // 记录成功请求（非流式）
                            tracker.complete(
                                provider.clone(),
                                model.clone(),
                                is_stream,
                                status_code,
                                prompt_tokens,
                                completion_tokens,
                            ).await;

                            final_response.into_response()
                        }
                        Err(e) => {
                            // 处理provider返回的错误
                            let error_message = format!("Provider error: {}", e);
                            tracing::error!("[{}] {}", request_id, error_message);
                            
                            // 记录失败请求
                            tracker.complete_error(
                                provider.clone(),
                                model.clone(),
                                is_stream,
                                502,
                                error_message.clone(),
                            ).await;
                            
                            (axum::http::StatusCode::BAD_GATEWAY, error_message).into_response()
                        }
                    }
                }
                Err(e) => {
                    // 处理超时错误
                    let is_timeout = e.to_string().contains("deadline has elapsed") || 
                                    std::any::type_name::<tokio::time::error::Elapsed>() == std::any::type_name_of_val(&e);
                    
                    let (status_code, error_message) = if is_timeout {
                        (504, "Request timeout".to_string())
                    } else {
                        (500, e.to_string())
                    };
                    
                    tracing::error!("[{}] Error forwarding request: {}", request_id, error_message);
                    
                    // 记录失败请求
                    tracker.complete_error(
                        provider.clone(),
                        model.clone(),
                        is_stream,
                        status_code,
                        error_message.clone(),
                    ).await;
                    
                    let status = if status_code == 504 {
                        axum::http::StatusCode::GATEWAY_TIMEOUT
                    } else {
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    };
                    
                    (status, error_message).into_response()
                }
            }
        }
        None => {
            tracing::error!("[{}] Provider not found: {}", request_id, provider);
            
            // 记录提供商未找到错误
            let tracker = request_logger.start_request(request_id.clone());
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
