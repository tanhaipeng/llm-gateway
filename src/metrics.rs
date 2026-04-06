use serde::{Deserialize, Serialize};

/// 性能监控器
#[derive(Clone)]
pub struct MetricsCollector {}

/// 性能指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_requests: u64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub avg_latency_ms: f64,
    /// H-3: 无延迟直方图时，这些字段值与 avg 相同（如需真实百分位，需维护滑动窗口）
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub requests_per_second: f64,
    pub total_tokens: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub avg_tokens_per_request: f64,
    pub requests_by_provider: std::collections::HashMap<String, ProviderMetrics>,
    pub uptime_seconds: u64,
}

/// 提供商特定指标 — H-1/H-2/H-3: 使用真实 per-provider 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {}
    }

    /// 使用指定的 logger 收集性能指标
    pub async fn collect_metrics_with_logger(
        &self,
        logger: &crate::logging::RequestLogger,
        uptime_seconds: u64,
    ) -> PerformanceMetrics {
        let stats = logger.get_stats().await;

        let total_requests = stats.total_requests;
        let success_rate = if total_requests > 0 {
            (stats.successful_requests as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };

        let error_rate = if total_requests > 0 {
            (stats.failed_requests as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };

        let avg_latency_ms = if total_requests > 0 {
            stats.total_duration_ms as f64 / total_requests as f64
        } else {
            0.0
        };

        let avg_tokens_per_request = if total_requests > 0 {
            stats.total_tokens as f64 / total_requests as f64
        } else {
            0.0
        };

        let requests_per_second = if uptime_seconds > 0 {
            total_requests as f64 / uptime_seconds as f64
        } else {
            0.0
        };

        // H-1/H-2/H-3: 使用真实 per-provider 统计，而不是全局值均摊
        let mut requests_by_provider = std::collections::HashMap::new();
        for (provider, ps) in &stats.stats_by_provider {
            let provider_success_rate = if ps.total_requests > 0 {
                (ps.successful_requests as f64 / ps.total_requests as f64) * 100.0
            } else {
                0.0
            };
            let provider_avg_latency = if ps.total_requests > 0 {
                ps.total_duration_ms as f64 / ps.total_requests as f64
            } else {
                0.0
            };
            requests_by_provider.insert(
                provider.clone(),
                ProviderMetrics {
                    total_requests: ps.total_requests,
                    successful_requests: ps.successful_requests,
                    failed_requests: ps.failed_requests,
                    success_rate: provider_success_rate,
                    avg_latency_ms: provider_avg_latency,
                    total_tokens: ps.total_tokens,
                    total_prompt_tokens: ps.total_prompt_tokens,
                    total_completion_tokens: ps.total_completion_tokens,
                },
            );
        }

        PerformanceMetrics {
            total_requests,
            success_rate,
            error_rate,
            avg_latency_ms,
            // H-3: 无延迟直方图，均设为 avg（诚实地表示无法计算真实百分位）
            p50_latency_ms: avg_latency_ms,
            p95_latency_ms: avg_latency_ms,
            p99_latency_ms: avg_latency_ms,
            requests_per_second,
            total_tokens: stats.total_tokens,
            total_prompt_tokens: stats.total_prompt_tokens,
            total_completion_tokens: stats.total_completion_tokens,
            avg_tokens_per_request,
            requests_by_provider,
            uptime_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = MetricsCollector::new();
        let logger = crate::logging::RequestLogger::new();

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

        let tracker = logger.start_request("test-2".to_string());
        tracker
            .complete_error(
                "openai".to_string(),
                "gpt-4".to_string(),
                true,
                500,
                "Internal error".to_string(),
            )
            .await;

        let metrics = collector.collect_metrics_with_logger(&logger, 60).await;

        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.success_rate, 50.0);
        assert_eq!(metrics.error_rate, 50.0);

        // H-1: per-provider token counts should be correct, not global-average
        let anthropic = &metrics.requests_by_provider["anthropic"];
        assert_eq!(anthropic.total_requests, 1);
        assert_eq!(anthropic.successful_requests, 1);
        assert_eq!(anthropic.total_tokens, 300);
        assert_eq!(anthropic.total_prompt_tokens, 100);
        assert_eq!(anthropic.total_completion_tokens, 200);

        let openai = &metrics.requests_by_provider["openai"];
        assert_eq!(openai.total_requests, 1);
        assert_eq!(openai.failed_requests, 1);
        assert_eq!(openai.total_tokens, 0);

        // H-2: per-provider success_rate should not be global value
        assert_eq!(anthropic.success_rate, 100.0);
        assert_eq!(openai.success_rate, 0.0);

        // M-5: global prompt/completion tracked
        assert_eq!(metrics.total_prompt_tokens, 100);
        assert_eq!(metrics.total_completion_tokens, 200);
    }
}
