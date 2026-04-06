use std::time::Instant;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 请求日志记录器
#[derive(Clone)]
pub struct RequestLogger {
    stats: Arc<RwLock<RequestStats>>,
}

/// 请求统计信息
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RequestStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_tokens: u64,
    pub total_duration_ms: u64,
    pub requests_by_provider: std::collections::HashMap<String, u64>,
    pub requests_by_status: std::collections::HashMap<String, u64>,
}

/// 单个请求日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLog {
    pub request_id: String,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub is_stream: bool,
    pub status_code: u16,
    pub duration_ms: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub error_message: Option<String>,
}

impl Default for RequestLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestLogger {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(RequestStats::default())),
        }
    }

    /// 开始记录请求
    pub fn start_request(&self, request_id: String) -> RequestTracker {
        RequestTracker {
            request_id,
            start_time: Instant::now(),
            logger: self.clone(),
        }
    }

    /// 记录请求成功
    pub async fn record_success(
        &self,
        request_id: String,
        provider: String,
        model: String,
        is_stream: bool,
        status_code: u16,
        duration_ms: u64,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) {
        let total_tokens = prompt_tokens + completion_tokens;
        
        let log = RequestLog {
            request_id,
            timestamp: Utc::now(),
            provider,
            model,
            is_stream,
            status_code,
            duration_ms,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            error_message: None,
        };

        self.log_request(log).await;
    }

    /// 记录请求失败
    pub async fn record_error(
        &self,
        request_id: String,
        provider: String,
        model: String,
        is_stream: bool,
        status_code: u16,
        duration_ms: u64,
        error_message: String,
    ) {
        let log = RequestLog {
            request_id,
            timestamp: Utc::now(),
            provider,
            model,
            is_stream,
            status_code,
            duration_ms,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            error_message: Some(error_message),
        };

        self.log_request(log).await;
    }

    /// 内部记录请求
    async fn log_request(&self, log: RequestLog) {
        // M-4: 先在独立作用域内持有写锁并更新统计，写锁释放后再调用 tracing::info!
        // 避免持锁时调用可能阻塞的日志 I/O
        let (request_id, provider_name, model, status_code, duration_ms, total_tokens, error) = {
            let mut stats = self.stats.write().await;

            stats.total_requests += 1;

            if log.error_message.is_none() {
                stats.successful_requests += 1;
                stats.total_tokens += log.total_tokens;
            } else {
                stats.failed_requests += 1;
            }

            stats.total_duration_ms += log.duration_ms;

            // 按提供商统计
            *stats.requests_by_provider.entry(log.provider.clone()).or_insert(0) += 1;

            // 按状态码统计
            *stats.requests_by_status.entry(log.status_code.to_string()).or_insert(0) += 1;

            // 写锁作用域结束前收集需要打印的信息
            (
                log.request_id.clone(),
                log.provider.clone(),
                log.model.clone(),
                log.status_code,
                log.duration_ms,
                log.total_tokens,
                log.error_message.clone(),
            )
        }; // 写锁在此释放

        // 写锁已释放，再输出日志
        tracing::info!(
            request_id = %request_id,
            provider = %provider_name,
            model = %model,
            status = status_code,
            duration_ms = duration_ms,
            tokens = total_tokens,
            error = error.as_deref().unwrap_or("none"),
            "Request completed"
        );
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> RequestStats {
        self.stats.read().await.clone()
    }

    /// 补充 token 统计（用于流式请求在流结束后异步更新）
    pub async fn add_tokens(&self, prompt_tokens: u64, completion_tokens: u64) {
        let total = prompt_tokens + completion_tokens;
        if total > 0 {
            let mut stats = self.stats.write().await;
            stats.total_tokens += total;
            tracing::debug!(
                prompt_tokens = prompt_tokens,
                completion_tokens = completion_tokens,
                "Stream token usage recorded"
            );
        }
    }
}

/// 请求跟踪器
pub struct RequestTracker {
    request_id: String,
    start_time: Instant,
    logger: RequestLogger,
}

impl RequestTracker {
    /// 完成请求（成功）
    pub async fn complete(
        self,
        provider: String,
        model: String,
        is_stream: bool,
        status_code: u16,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        self.logger
            .record_success(
                self.request_id,
                provider,
                model,
                is_stream,
                status_code,
                duration_ms,
                prompt_tokens,
                completion_tokens,
            )
            .await;
    }

    /// 完成请求（失败）
    pub async fn complete_error(
        self,
        provider: String,
        model: String,
        is_stream: bool,
        status_code: u16,
        error_message: String,
    ) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        self.logger
            .record_error(
                self.request_id,
                provider,
                model,
                is_stream,
                status_code,
                duration_ms,
                error_message,
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_request_logger() {
        let logger = RequestLogger::new();
        
        // 记录成功请求
        let tracker = logger.start_request("test-1".to_string());
        tracker
            .complete(
                "anthropic".to_string(),
                "claude-3-5-sonnet".to_string(),
                false,
                200,
                100,
                200,
            )
            .await;

        let stats = logger.get_stats().await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.failed_requests, 0);
        assert_eq!(stats.total_tokens, 300);
    }

    #[tokio::test]
    async fn test_request_error() {
        let logger = RequestLogger::new();
        
        // 记录失败请求
        let tracker = logger.start_request("test-2".to_string());
        tracker
            .complete_error(
                "openai".to_string(),
                "gpt-4".to_string(),
                true,
                500,
                "Internal server error".to_string(),
            )
            .await;

        let stats = logger.get_stats().await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.successful_requests, 0);
        assert_eq!(stats.failed_requests, 1);
    }
}