use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// 请求日志记录器
#[derive(Clone)]
pub struct RequestLogger {
    stats: Arc<RwLock<RequestStats>>,
}

/// 提供商级别的统计信息
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProviderStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_tokens: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_duration_ms: u64,
}

/// 请求统计信息
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RequestStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_tokens: u64,
    /// M-5: 分别追踪 prompt 和 completion token
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_duration_ms: u64,
    pub requests_by_provider: std::collections::HashMap<String, u64>,
    pub requests_by_status: std::collections::HashMap<String, u64>,
    /// H-1/H-2/H-3: 真实的 per-provider 统计数据
    pub stats_by_provider: std::collections::HashMap<String, ProviderStats>,
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

        // M-4: 先在独立作用域内持有写锁并更新统计，写锁释放后再调用 tracing::info!
        {
            let mut stats = self.stats.write().await;

            stats.total_requests += 1;
            stats.successful_requests += 1;
            stats.total_tokens += total_tokens;
            // M-5: 分别记录 prompt / completion
            stats.total_prompt_tokens += prompt_tokens;
            stats.total_completion_tokens += completion_tokens;
            stats.total_duration_ms += duration_ms;

            *stats
                .requests_by_provider
                .entry(provider.clone())
                .or_insert(0) += 1;
            *stats
                .requests_by_status
                .entry(status_code.to_string())
                .or_insert(0) += 1;

            // H-1/H-2/H-3: 更新 per-provider 详细统计
            let ps = stats.stats_by_provider.entry(provider.clone()).or_default();
            ps.total_requests += 1;
            ps.successful_requests += 1;
            ps.total_tokens += total_tokens;
            ps.total_prompt_tokens += prompt_tokens;
            ps.total_completion_tokens += completion_tokens;
            ps.total_duration_ms += duration_ms;
        } // 写锁在此释放

        tracing::info!(
            request_id = %request_id,
            provider = %provider,
            model = %model,
            status = status_code,
            duration_ms = duration_ms,
            prompt_tokens = prompt_tokens,
            completion_tokens = completion_tokens,
            is_stream = is_stream,
            "Request completed"
        );
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
        {
            let mut stats = self.stats.write().await;

            stats.total_requests += 1;
            stats.failed_requests += 1;
            stats.total_duration_ms += duration_ms;

            *stats
                .requests_by_provider
                .entry(provider.clone())
                .or_insert(0) += 1;
            *stats
                .requests_by_status
                .entry(status_code.to_string())
                .or_insert(0) += 1;

            // H-1/H-2/H-3: 更新 per-provider 失败统计
            let ps = stats.stats_by_provider.entry(provider.clone()).or_default();
            ps.total_requests += 1;
            ps.failed_requests += 1;
            ps.total_duration_ms += duration_ms;
        } // 写锁在此释放

        tracing::warn!(
            request_id = %request_id,
            provider = %provider,
            model = %model,
            status = status_code,
            duration_ms = duration_ms,
            is_stream = is_stream,
            error = %error_message,
            "Request failed"
        );
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> RequestStats {
        self.stats.read().await.clone()
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
        // M-5: prompt/completion tracked separately
        assert_eq!(stats.total_prompt_tokens, 100);
        assert_eq!(stats.total_completion_tokens, 200);
        // per-provider stats
        let ps = &stats.stats_by_provider["anthropic"];
        assert_eq!(ps.total_requests, 1);
        assert_eq!(ps.total_tokens, 300);
        assert_eq!(ps.total_prompt_tokens, 100);
        assert_eq!(ps.total_completion_tokens, 200);
    }

    #[tokio::test]
    async fn test_request_error() {
        let logger = RequestLogger::new();

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
        let ps = &stats.stats_by_provider["openai"];
        assert_eq!(ps.failed_requests, 1);
    }
}
