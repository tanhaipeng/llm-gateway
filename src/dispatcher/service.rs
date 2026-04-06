use crate::types::{Config, Provider, ProviderConfig, SSEStream};
use crate::mapper::{RequestMapper, ResponseMapper};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct Dispatcher {
    providers: Arc<HashMap<String, ProviderClient>>,
}

#[derive(Clone)]
pub struct ProviderClient {
    client: reqwest::Client,
    config: Arc<ProviderConfig>,
    provider: Provider,
}

impl ProviderClient {
    pub fn new(provider: Provider, config: ProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            // 连接池配置
            .pool_max_idle_per_host(10) // 每个host最大空闲连接数
            .pool_idle_timeout(std::time::Duration::from_secs(90)) // 空闲连接超时
            .http2_keep_alive_interval(std::time::Duration::from_secs(30)) // HTTP2 keep-alive
            .http2_keep_alive_timeout(std::time::Duration::from_secs(10)) // HTTP2 keep-alive超时
            .http2_keep_alive_while_idle(true) // 空闲时保持HTTP2连接
            // 超时配置
            .timeout(std::time::Duration::from_secs(900))
            .connect_timeout(std::time::Duration::from_secs(30)) // 连接超时
            .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
            // 重试配置
            .redirect(reqwest::redirect::Policy::limited(5)) // 最多重定向5次
            // 性能优化
            .tcp_nodelay(true) // 禁用Nagle算法
            .tcp_keepalive(std::time::Duration::from_secs(60)) // TCP keep-alive
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config: Arc::new(config),
            provider,
        }
    }

    /// 构建请求URL
    fn build_url(&self) -> String {
        if self.provider == Provider::Anthropic {
            format!("{}/v1/messages", self.config.base_url)
        } else {
            format!("{}/v1/chat/completions", self.config.base_url)
        }
    }

    /// 添加provider特定的headers
    fn add_provider_headers(&self, mut request_builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.provider {
            Provider::OpenAI | Provider::GoogleGemini | Provider::Deepseek | Provider::Custom(_) => {
                if let Some(api_key) = &self.config.api_key {
                    request_builder = request_builder.header(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {}", api_key),
                    );
                }
            }
            Provider::Anthropic => {
                if let Some(api_key) = &self.config.api_key {
                    request_builder = request_builder.header("x-api-key", api_key);
                }
                if let Some(version) = &self.config.version {
                    request_builder = request_builder.header("anthropic-version", version);
                }
            }
        }
        request_builder
    }

    /// 非流式请求转发
    pub async fn forward_request(
        &self,
        body: bytes::Bytes,
    ) -> Result<axum::response::Response, crate::types::GatewayError> {
        let converted_body = RequestMapper::convert_request(&body, &self.provider)?;
        let url = self.build_url();
        
        let mut request_builder = self.add_provider_headers(self.client.post(&url));
        
        request_builder = request_builder
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(converted_body);

        let response = request_builder.send().await?;
        
        let status = response.status();
        let headers = response.headers().clone();
        
        // 处理错误响应
        if !status.is_success() {
            let error_body = match response.text().await {
                Ok(text) => text,
                Err(_) => format!("HTTP {}: {}", status, status.canonical_reason().unwrap_or("Unknown")),
            };
            
// 尝试解析错误响应
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                if let Some(error_obj) = json.get("error") {
                    if let Some(error_message) = error_obj.get("message").and_then(|m| m.as_str()) {
                        let error_response = serde_json::json!({
                            "error": {
                                "message": error_message,
                                "type": error_obj.get("type").and_then(|t| t.as_str()).unwrap_or("api_error"),
                                "code": error_obj.get("code").and_then(|c| c.as_str()).unwrap_or("")
                            }
                        });
                        return Ok(axum::response::Response::builder()
                            .status(status)
                            .header("Content-Type", "application/json")
                            .body(axum::body::Body::from(serde_json::to_vec(&error_response)
                                .expect("Failed to serialize error response")))
                            .expect("Failed to build error response")
                        );
                    }
                }
            }
            
            // 返回原始错误文本
            let error_response = serde_json::json!({
                "error": {
                    "message": error_body,
                    "type": "api_error",
                    "code": status.as_u16().to_string()
                }
            });

            return Ok(axum::response::Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&error_response)
                    .expect("Failed to serialize error response")))
                .expect("Failed to build error response")
            );
        }

        // 处理成功响应
        let body_bytes = response.bytes().await?;
        let response_data = String::from_utf8_lossy(&body_bytes);
        
        // 转换响应格式
        let converted_response = match ResponseMapper::convert_response(&response_data, &self.provider, false) {
            Ok(response) => response,
            Err(e) => {
                tracing::error!("Failed to convert response: {}", e);
                // 如果转换失败，返回原始响应
                response_data.to_string()
            }
        };
        
        let mut axum_response = axum::response::Response::builder()
            .status(status);
        
        // 只复制必要的响应头
        for (name, value) in headers.iter() {
            // 跳过一些不应转发的头
            if !name.as_str().eq_ignore_ascii_case("content-length") 
                && !name.as_str().eq_ignore_ascii_case("content-encoding")
                && !name.as_str().eq_ignore_ascii_case("transfer-encoding")
                && !name.as_str().eq_ignore_ascii_case("connection")
                && !name.as_str().eq_ignore_ascii_case("server")
                && !name.as_str().eq_ignore_ascii_case("date")
            {
                axum_response = axum_response.header(name, value);
            }
        }
        
        let response = axum_response.body(axum::body::Body::from(converted_response))?;
        Ok(response)
    }

    /// 流式请求转发
    pub async fn forward_request_stream(
        &self,
        body: bytes::Bytes,
    ) -> Result<(axum::response::Response, std::sync::Arc<tokio::sync::Mutex<(u64, u64)>>), crate::types::GatewayError> {
        let converted_body = RequestMapper::convert_request(&body, &self.provider)?;
        let url = self.build_url();
        let provider_clone = self.provider.clone();
        
        let mut request_builder = self.add_provider_headers(self.client.post(&url));

        request_builder = request_builder
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(converted_body);

        let response = request_builder.send().await?;
        let status = response.status();
        
        // 处理流式请求的错误响应
        if !status.is_success() {
            let error_body = match response.text().await {
                Ok(text) => text,
                Err(_) => format!("HTTP {}: {}", status, status.canonical_reason().unwrap_or("Unknown")),
            };

            tracing::error!("Provider returned error in stream request {}: {}", status, error_body);

            // 尝试解析错误响应
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                if let Some(error_obj) = json.get("error") {
                    if let Some(error_message) = error_obj.get("message").and_then(|m| m.as_str()) {
                        let error_response = serde_json::json!({
                            "error": {
                                "message": error_message,
                                "type": error_obj.get("type").and_then(|t| t.as_str()).unwrap_or("api_error"),
                                "code": error_obj.get("code").and_then(|c| c.as_str()).unwrap_or("")
                            }
                        });
                        let empty_counter = std::sync::Arc::new(tokio::sync::Mutex::new((0u64, 0u64)));
                        return Ok((axum::response::Response::builder()
                            .status(status)
                            .header("Content-Type", "application/json")
                            .body(axum::body::Body::from(serde_json::to_vec(&error_response)
                                .expect("Failed to serialize error response")))
                            .expect("Failed to build error response"),
                            empty_counter,
                        ));
                    }
                }
            }

            // 返回原始错误文本
            let error_response = serde_json::json!({
                "error": {
                    "message": error_body,
                    "type": "api_error",
                    "code": status.as_u16().to_string()
                }
            });

            let empty_counter = std::sync::Arc::new(tokio::sync::Mutex::new((0u64, 0u64)));
            return Ok((axum::response::Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&error_response)
                    .expect("Failed to serialize error response")))
                .expect("Failed to build error response"),
                empty_counter,
            ));
        }

        // 用于在流处理过程中捕获 token 使用量
        let token_counter: std::sync::Arc<tokio::sync::Mutex<(u64, u64)>> =
            std::sync::Arc::new(tokio::sync::Mutex::new((0u64, 0u64)));
        let token_counter_clone = token_counter.clone();

        // 处理流式响应
        let byte_stream = response.bytes_stream().map(|result| {
            result.map_err(|e| {
                tracing::warn!("Stream error: {}", e);
                crate::types::GatewayError::StreamError(
                    crate::types::stream::StreamError::BodyError(e.to_string())
                )
            })
        });

        let converted_stream = Box::pin(byte_stream.then(move |result| {
            let token_counter_inner = token_counter_clone.clone();
            let provider_clone = provider_clone.clone();
            async move {
                result.and_then(|bytes| {
                    // 处理空数据块
                    if bytes.is_empty() {
                        return Ok(bytes::Bytes::new());
                    }

                    let data = String::from_utf8_lossy(&bytes);

                    // 转换流式数据
                    match ResponseMapper::convert_response(&data, &provider_clone, true) {
                        Ok(converted) => {
                            // 检查是否是结束标记
                            if converted.trim() == "[DONE]" {
                                return Ok(bytes::Bytes::from("data: [DONE]\n\n"));
                            }

                            // 尝试从转换后的 JSON chunk 中提取 usage 字段
                            // 流式最后一个包含 usage 的非 null chunk 记录了 token 消耗
                            let json_str = if converted.starts_with("data:") {
                                converted.trim_start_matches("data:").trim().to_string()
                            } else {
                                converted.clone()
                            };
                            if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                if let Some(usage) = chunk_json.get("usage") {
                                    if !usage.is_null() {
                                        let prompt = usage.get("prompt_tokens")
                                            .and_then(|t| t.as_u64())
                                            .unwrap_or(0);
                                        let completion = usage.get("completion_tokens")
                                            .and_then(|t| t.as_u64())
                                            .unwrap_or(0);
                                        if prompt > 0 || completion > 0 {
                                            // 用 try_lock 避免 async 闭包中的 await 复杂性
                                            if let Ok(mut counter) = token_counter_inner.try_lock() {
                                                *counter = (prompt, completion);
                                            }
                                        }
                                    }
                                }
                            }

                            // 检查转换后的数据是否已经是 SSE 格式（包含 "data:" 前缀）
                            // 如果已经包含，直接返回原始数据（保留原有的换行符）
                            // 否则添加 "data: " 前缀和换行符
                            if converted.starts_with("data:") {
                                Ok(bytes::Bytes::from(converted))
                            } else {
                                Ok(bytes::Bytes::from(format!("data: {}\n\n", converted)))
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to convert stream data: {}", e);
                            // 转换失败时，检查原始数据是否已经是 SSE 格式
                            if data.starts_with("data:") {
                                Ok(bytes::Bytes::from(data.to_string()))
                            } else {
                                Ok(bytes::Bytes::from(format!("data: {}\n\n", data)))
                            }
                        }
                    }
                })
            }
        })) as SSEStream;

        let axum_response = axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "text/event-stream; charset=utf-8")
            .header("Cache-Control", "no-cache, no-transform")
            .header("Connection", "keep-alive")
            .header("X-Accel-Buffering", "no"); // 禁用nginx缓冲

        let body = axum::body::Body::from_stream(converted_stream);
        Ok((axum_response.body(body)?, token_counter))
    }
}

impl Dispatcher {
    pub fn new(config: &Config) -> Self {
        let mut providers = HashMap::new();
        
        for (name, provider_config) in &config.providers {
            let provider = Provider::from_str(name)
                .unwrap_or_else(|_| Provider::Custom(name.clone()));
            
            let client = ProviderClient::new(provider, provider_config.clone());
            providers.insert(name.clone(), client);
        }
        
        Self {
            providers: Arc::new(providers),
        }
    }

    pub fn get_provider(&self, name: &str) -> Option<&ProviderClient> {
        self.providers.get(name)
    }
}
