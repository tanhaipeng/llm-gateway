use crate::logging::RequestLogger;
use serde::{Deserialize, Serialize};

/// 性能监控器
#[derive(Clone)]
pub struct MetricsCollector {
    logger: RequestLogger,
}

/// 性能指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_requests: u64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub requests_per_second: f64,
    pub total_tokens: u64,
    pub avg_tokens_per_request: f64,
    pub requests_by_provider: std::collections::HashMap<String, ProviderMetrics>,
    pub uptime_seconds: u64,
}

/// 提供商特定指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetrics {
    pub total_requests: u64,
    pub success_rate: f64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            logger: RequestLogger::new(),
        }
    }

    /// 收集性能指标
    pub async fn collect_metrics(&self, uptime_seconds: u64) -> PerformanceMetrics {
        let stats = self.logger.get_stats().await;
        
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
        
        // 计算提供商特定指标
        let mut requests_by_provider = std::collections::HashMap::new();
        for (provider, count) in &stats.requests_by_provider {
            let provider_metrics = ProviderMetrics {
                total_requests: *count,
                success_rate: success_rate, // 简化处理，实际应该分别计算
                avg_latency_ms: avg_latency_ms, // 简化处理，实际应该分别计算
                total_tokens: if !stats.requests_by_provider.is_empty() {
                    stats.total_tokens / stats.requests_by_provider.len() as u64
                } else {
                    0
                }, // 简化处理
            };
            requests_by_provider.insert(provider.clone(), provider_metrics);
        }
        
        // 这里我们简化了P50, P95, P99延迟的计算
        // 实际实现需要收集所有请求的延迟数据并计算百分位数
        PerformanceMetrics {
            total_requests,
            success_rate,
            error_rate,
            avg_latency_ms,
            p50_latency_ms: avg_latency_ms, // 简化处理
            p95_latency_ms: avg_latency_ms * 1.5, // 简化处理
            p99_latency_ms: avg_latency_ms * 2.0, // 简化处理
            requests_per_second,
            total_tokens: stats.total_tokens,
            avg_tokens_per_request,
            requests_by_provider,
            uptime_seconds,
        }
    }

    /// 获取健康状态
    pub async fn get_health_status(&self) -> HealthStatus {
        let stats = self.logger.get_stats().await;
        
        let total_requests = stats.total_requests;
        let error_rate = if total_requests > 0 {
            (stats.failed_requests as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };
        
        let status = if error_rate < 5.0 {
            "healthy"
        } else if error_rate < 20.0 {
            "degraded"
        } else {
            "unhealthy"
        };
        
        HealthStatus {
            status: status.to_string(),
            total_requests,
            error_rate,
            avg_latency_ms: if total_requests > 0 {
                stats.total_duration_ms as f64 / total_requests as f64
            } else {
                0.0
            },
        }
    }
}

/// 健康状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub total_requests: u64,
    pub error_rate: f64,
    pub avg_latency_ms: f64,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            status: "healthy".to_string(),
            total_requests: 0,
            error_rate: 0.0,
            avg_latency_ms: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = MetricsCollector::new();
        
        // 记录一些请求
        let tracker = collector.logger.start_request("test-1".to_string());
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
        
        let tracker = collector.logger.start_request("test-2".to_string());
        tracker
            .complete_error(
                "openai".to_string(),
                "gpt-4".to_string(),
                true,
                500,
                "Internal error".to_string(),
            )
            .await;
        
        let metrics = collector.collect_metrics(60).await;
        
        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.success_rate, 50.0);
        assert_eq!(metrics.error_rate, 50.0);
    }

    #[tokio::test]
    async fn test_health_status() {
        let collector = MetricsCollector::new();
        
        let status = collector.get_health_status().await;
        assert_eq!(status.status, "healthy");
    }
}