use crate::mapper::response::StreamState;
use crate::mapper::{RequestMapper, ResponseMapper};
use crate::types::{Config, GatewayError, Provider, ProviderConfig, SSEStream};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::Notify;

const MAX_SSE_BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// 流式 token 计数器（AtomicU64 避免锁竞争）
/// `done` 标志在流完全处理完毕后置 true，让后台任务立即退出而不是等 900s
pub struct StreamTokenCounter {
    pub prompt: AtomicU64,
    pub completion: AtomicU64,
    /// 流处理中是否出现错误（读流错误、provider error 事件等）
    pub errored: AtomicBool,
    /// 流处理完毕信号（无论成功/错误/[DONE]）
    pub done: AtomicBool,
    /// 用于通知等待方（router）流已结束，避免轮询
    pub notify: Notify,
}

impl StreamTokenCounter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            prompt: AtomicU64::new(0),
            completion: AtomicU64::new(0),
            errored: AtomicBool::new(false),
            done: AtomicBool::new(false),
            notify: Notify::new(),
        })
    }

    fn mark_done(&self) {
        self.done.store(true, Ordering::Release);
        // 使用 notify_one 保留 permit，避免先完成后等待时丢唤醒
        self.notify.notify_one();
    }

    fn mark_error(&self) {
        self.errored.store(true, Ordering::Release);
        self.mark_done();
    }
}

fn append_sse_text_with_limit(buf: &mut String, text: &str) -> Result<(), GatewayError> {
    // 统一换行符，兼容 \r\n 的 SSE 实现
    if text.contains('\r') {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        buf.push_str(&normalized);
    } else {
        buf.push_str(text);
    }

    if buf.len() > MAX_SSE_BUFFER_BYTES {
        return Err(GatewayError::StreamError(
            crate::types::stream::StreamError::BodyError("SSE buffer exceeded limit".to_string()),
        ));
    }
    Ok(())
}

struct StreamCompletionGuard {
    counter: Arc<StreamTokenCounter>,
}

impl Drop for StreamCompletionGuard {
    fn drop(&mut self) {
        // 无论是正常结束还是客户端提前断开，只要流对象被 drop，就应结束等待
        self.counter.mark_done();
    }
}

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
    /// M-1: 返回 Result 而不是 expect panic
    pub fn new(provider: Provider, config: ProviderConfig) -> Result<Self, GatewayError> {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .timeout(std::time::Duration::from_secs(900))
            .connect_timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .tcp_nodelay(true)
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| GatewayError::HttpError(e))?;

        Ok(Self {
            client,
            config: Arc::new(config),
            provider,
        })
    }

    fn build_url(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if self.uses_responses_protocol() {
            return format!("{}/v1/responses", base);
        }
        if self.provider == Provider::Anthropic {
            format!("{}/v1/messages", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }

    fn uses_responses_protocol(&self) -> bool {
        self.config
            .protocol
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("responses"))
    }

    fn add_provider_headers(
        &self,
        mut request_builder: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        match self.provider {
            Provider::OpenAI
            | Provider::GoogleGemini
            | Provider::Deepseek
            | Provider::Custom(_) => {
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

    /// 构建统一的错误响应（透传真实 HTTP 状态码）
    /// H-6: error_body 截断到 512 字符，避免将 provider 内部敏感信息写入日志
    fn build_error_response(
        status: reqwest::StatusCode,
        error_body: &str,
    ) -> axum::response::Response {
        // H-6: 日志中截断错误体，避免泄露敏感内容
        let truncated = if error_body.len() > 512 {
            format!("{}…[truncated]", &error_body[..512])
        } else {
            error_body.to_string()
        };
        tracing::warn!(status = %status, body = %truncated, "Provider returned error");

        let body = if let Ok(json) = serde_json::from_str::<serde_json::Value>(error_body) {
            if let Some(error_obj) = json.get("error") {
                serde_json::json!({
                    "error": {
                        "message": error_obj.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Provider error"),
                        "type": error_obj.get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("api_error"),
                        "code": error_obj.get("code")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                    }
                })
            } else {
                serde_json::json!({"error": {"message": "Provider error", "type": "api_error", "code": status.as_u16().to_string()}})
            }
        } else {
            serde_json::json!({"error": {"message": "Provider error", "type": "api_error", "code": status.as_u16().to_string()}})
        };

        axum::response::Response::builder()
            .status(status.as_u16())
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&body).unwrap_or_default(),
            ))
            .unwrap_or_else(|_| {
                axum::response::Response::builder()
                    .status(500)
                    .body(axum::body::Body::empty())
                    .unwrap()
            })
    }

    /// 非流式请求转发
    pub async fn forward_request(
        &self,
        body: bytes::Bytes,
    ) -> Result<axum::response::Response, GatewayError> {
        let converted_body = RequestMapper::convert_request_by_protocol(
            &body,
            &self.provider,
            self.uses_responses_protocol(),
        )?;
        let url = self.build_url();

        let request_builder = self
            .add_provider_headers(self.client.post(&url))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(converted_body);

        let response = request_builder.send().await?;
        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_else(|e| e.to_string());
            return Ok(Self::build_error_response(status, &error_body));
        }

        let body_bytes = response.bytes().await?;

        // C-3: 使用 from_utf8 而不是 from_utf8_lossy，避免静默修改损坏字节
        let response_data = match std::str::from_utf8(&body_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                tracing::warn!("Provider response is not valid UTF-8, passing through raw bytes");
                return Ok(axum::response::Response::builder()
                    .status(status)
                    .header("Content-Type", "application/octet-stream")
                    .body(axum::body::Body::from(body_bytes))?);
            }
        };

        let converted_response = match ResponseMapper::convert_response_by_protocol(
            &response_data,
            &self.provider,
            self.uses_responses_protocol(),
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to convert response, passing through raw");
                response_data
            }
        };

        let mut axum_response = axum::response::Response::builder()
            .status(status)
            .header("Content-Type", "application/json");

        for (name, value) in headers.iter() {
            let n = name.as_str();
            if !n.eq_ignore_ascii_case("content-length")
                && !n.eq_ignore_ascii_case("content-encoding")
                && !n.eq_ignore_ascii_case("transfer-encoding")
                && !n.eq_ignore_ascii_case("connection")
                && !n.eq_ignore_ascii_case("server")
                && !n.eq_ignore_ascii_case("date")
                && !n.eq_ignore_ascii_case("content-type")
            {
                axum_response = axum_response.header(name, value);
            }
        }

        Ok(axum_response.body(axum::body::Body::from(converted_response))?)
    }

    /// 流式请求转发
    /// 返回 (Response, StreamTokenCounter)
    pub async fn forward_request_stream(
        &self,
        body: bytes::Bytes,
    ) -> Result<(axum::response::Response, Arc<StreamTokenCounter>), GatewayError> {
        let is_responses_protocol = self.uses_responses_protocol();
        let converted_body = RequestMapper::convert_request_by_protocol(
            &body,
            &self.provider,
            is_responses_protocol,
        )?;
        let url = self.build_url();
        let provider_clone = self.provider.clone();

        let request_builder = self
            .add_provider_headers(self.client.post(&url))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(converted_body);

        let response = request_builder.send().await?;
        let status = response.status();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_else(|e| e.to_string());
            let counter = StreamTokenCounter::new();
            // 标记为 done，后台任务不会等待
            counter.mark_done();
            return Ok((Self::build_error_response(status, &error_body), counter));
        }

        let token_counter = StreamTokenCounter::new();
        let token_counter_clone = token_counter.clone();

        let byte_stream = response.bytes_stream().map(|result| {
            result.map_err(|e| {
                tracing::warn!(error = %e, "Stream read error");
                GatewayError::StreamError(crate::types::stream::StreamError::BodyError(
                    e.to_string(),
                ))
            })
        });

        // 在字节流末尾追加哨兵，确保最后一帧被刷出并能标记 done
        let byte_stream_with_sentinel = byte_stream.chain(futures::stream::once(async {
            Ok::<bytes::Bytes, GatewayError>(bytes::Bytes::from(":__LLM_GATEWAY_EOF__\n\n"))
        }));

        let converted_stream = Box::pin({
            let mut buf = String::new();
            let mut stream_state = StreamState::new();
            let completion_guard = StreamCompletionGuard {
                counter: token_counter_clone.clone(),
            };

            byte_stream_with_sentinel.flat_map(move |result| {
                let _keep_guard_alive = &completion_guard;
                let token_counter_inner = token_counter_clone.clone();
                let provider = provider_clone.clone();

                match result {
                    Err(e) => {
                        // 流出错，标记 done 让后台任务立即退出
                        token_counter_inner.mark_error();
                        futures::stream::iter(vec![Err(e)])
                    }
                    Ok(bytes) => {
                        // C-3: 使用 from_utf8_lossy 仍然保留（SSE 数据本身必须是 UTF-8）
                        // 但记录警告当出现替换字符
                        let text = String::from_utf8_lossy(&bytes);
                        if text.contains('\u{FFFD}') {
                            tracing::warn!("Non-UTF-8 bytes in SSE stream, data may be corrupted");
                        }
                        if let Err(e) = append_sse_text_with_limit(&mut buf, &text) {
                            token_counter_inner.mark_error();
                            tracing::warn!(
                                current = buf.len(),
                                max = MAX_SSE_BUFFER_BYTES,
                                "SSE buffer exceeded limit"
                            );
                            return futures::stream::iter(vec![Err(e)]);
                        }

                        let mut output: Vec<Result<bytes::Bytes, GatewayError>> =
                            Vec::new();

                        // 按 \n\n 分割完整的 SSE 事件
                        while let Some(pos) = buf.find("\n\n") {
                            let frame = buf[..pos].to_string();
                            buf.drain(..pos + 2);

                            if frame.trim().is_empty() {
                                continue;
                            }

                            // 内部 EOF 哨兵：上游正常 EOF 也要显式结束，避免后台任务等待 900s
                            if frame.trim() == ":__LLM_GATEWAY_EOF__" {
                                if is_responses_protocol {
                                    output.push(Ok(bytes::Bytes::from("data: [DONE]\n\n")));
                                }
                                token_counter_inner.mark_done();
                                continue;
                            }

                            // 从 SSE 帧中提取全部 data: 行（SSE 允许多行 data）
                            let data_lines: Vec<&str> = frame
                                .lines()
                                .filter_map(|line| {
                                    line.strip_prefix("data:")
                                        .map(|s| s.strip_prefix(' ').unwrap_or(s))
                                })
                                .collect();
                            if data_lines.is_empty() {
                                continue;
                            }
                            let json_owned = data_lines.join("\n");
                            let json_str = json_owned.as_str();

                            // [DONE] 终止信号 — 标记流完毕
                            if json_str == "[DONE]" {
                                token_counter_inner.mark_done();
                                if !is_responses_protocol {
                                    output.push(Ok(bytes::Bytes::from("data: [DONE]\n\n")));
                                }
                                continue;
                            }

                            // provider 侧流式错误/结束事件
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                                if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                                    if t == "error" || t == "response.failed" {
                                        token_counter_inner.mark_error();
                                    } else if t == "message_stop" {
                                        token_counter_inner.mark_done();
                                    }
                                }
                            }

                            match ResponseMapper::convert_stream_chunk_by_protocol(
                                json_str,
                                &provider,
                                &mut stream_state,
                                is_responses_protocol,
                            ) {
                                Ok(None) => {}
                                Ok(Some(converted)) => {
                                    // C-1: 提取 token 使用量，条件改为 prompt > 0 || completion > 0
                                    if let Ok(chunk_json) =
                                        serde_json::from_str::<serde_json::Value>(&converted)
                                    {
                                        if let Some(usage) = chunk_json.get("usage") {
                                            if !usage.is_null() {
                                                let prompt = usage
                                                    .get("prompt_tokens")
                                                    .and_then(|t| t.as_u64())
                                                    .unwrap_or(0);
                                                let completion = usage
                                                    .get("completion_tokens")
                                                    .and_then(|t| t.as_u64())
                                                    .unwrap_or(0);
                                                // C-1 fix: 只要有任意 token 就记录
                                                if prompt > 0 || completion > 0 {
                                                    token_counter_inner
                                                        .prompt
                                                        .store(prompt, Ordering::Relaxed);
                                                    token_counter_inner
                                                        .completion
                                                        .store(completion, Ordering::Relaxed);
                                                }
                                            }
                                        }
                                    }
                                    output.push(Ok(bytes::Bytes::from(format!(
                                        "data: {}\n\n",
                                        converted
                                    ))));
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, raw = %json_str, "Failed to convert stream chunk, passing through");
                                    output.push(Ok(bytes::Bytes::from(format!(
                                        "data: {}\n\n",
                                        json_str
                                    ))));
                                }
                            }
                        }

                        futures::stream::iter(output)
                    }
                }
            })
        }) as SSEStream;

        let axum_response = axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "text/event-stream; charset=utf-8")
            .header("Cache-Control", "no-cache, no-transform")
            .header("Connection", "keep-alive")
            .header("X-Accel-Buffering", "no")
            .body(axum::body::Body::from_stream(converted_stream))?;

        Ok((axum_response, token_counter))
    }
}

impl Dispatcher {
    /// M-1: new 返回 Result，让启动时快速失败
    pub fn new(config: &Config) -> Result<Self, GatewayError> {
        let mut providers = HashMap::new();

        for (name, provider_config) in &config.providers {
            let provider = Provider::from_str(name)?;
            let client = ProviderClient::new(provider, provider_config.clone()).map_err(|e| {
                tracing::error!(provider = %name, error = %e, "Failed to create provider client");
                e
            })?;
            providers.insert(name.clone(), client);
        }

        Ok(Self {
            providers: Arc::new(providers),
        })
    }

    pub fn get_provider(&self, name: &str) -> Option<&ProviderClient> {
        self.providers.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_stream_counter_mark_done_notifies() {
        let counter = StreamTokenCounter::new();
        let notified = counter.notify.notified();
        counter.mark_done();
        notified.await;
        assert!(counter.done.load(Ordering::Acquire));
        assert!(!counter.errored.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_stream_counter_mark_error_sets_flags_and_notifies() {
        let counter = StreamTokenCounter::new();
        let notified = counter.notify.notified();
        counter.mark_error();
        notified.await;
        assert!(counter.done.load(Ordering::Acquire));
        assert!(counter.errored.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_stream_counter_mark_done_before_wait_is_not_lost() {
        let counter = StreamTokenCounter::new();
        counter.mark_done();
        let waited = timeout(Duration::from_millis(50), counter.notify.notified()).await;
        assert!(waited.is_ok());
        assert!(counter.done.load(Ordering::Acquire));
    }

    #[test]
    fn test_append_sse_text_with_limit_returns_error_when_exceeded() {
        let mut buf = "a".repeat(MAX_SSE_BUFFER_BYTES - 2);
        assert!(append_sse_text_with_limit(&mut buf, "xy").is_ok());
        let err = append_sse_text_with_limit(&mut buf, "z").unwrap_err();
        assert!(matches!(
            err,
            GatewayError::StreamError(crate::types::stream::StreamError::BodyError(_))
        ));
    }

    #[test]
    fn test_append_sse_text_with_limit_normalizes_crlf() {
        let mut buf = String::new();
        append_sse_text_with_limit(&mut buf, "data: 1\r\ndata: 2\r\r\n")
            .expect("append_sse_text_with_limit should normalize CRLF");
        assert_eq!(buf, "data: 1\ndata: 2\n\n");
    }
}
