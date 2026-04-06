use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Instant;

#[derive(Clone)]
pub struct RateLimitState {
    available_mtokens: Arc<AtomicU64>,
    last_refill_ms: Arc<AtomicU64>,
    refill_lock: Arc<Mutex<()>>,
    start_at: Instant,
    rate_per_second: u64,
    capacity_mtokens: u64,
}

enum AcquireResult {
    Allowed,
    Denied { retry_after_seconds: u64 },
}

impl RateLimitState {
    pub fn new(max_requests_per_second: u64) -> Self {
        // 1 token = 1000 mtokens
        let capacity_mtokens = max_requests_per_second.saturating_mul(1000);
        Self {
            available_mtokens: Arc::new(AtomicU64::new(capacity_mtokens)),
            last_refill_ms: Arc::new(AtomicU64::new(0)),
            refill_lock: Arc::new(Mutex::new(())),
            start_at: Instant::now(),
            rate_per_second: max_requests_per_second,
            capacity_mtokens,
        }
    }

    fn now_ms(&self, now: Instant) -> u64 {
        now.duration_since(self.start_at).as_millis() as u64
    }

    fn try_acquire(&self, now: Instant) -> AcquireResult {
        self.try_refill(now);

        // Lock-free consume path
        loop {
            let current = self.available_mtokens.load(Ordering::Relaxed);
            if current < 1000 {
                let missing_mtokens = 1000_u64.saturating_sub(current);
                let wait_ms = if self.rate_per_second == 0 {
                    1000
                } else {
                    missing_mtokens.div_ceil(self.rate_per_second)
                };
                let retry_after_seconds = wait_ms.div_ceil(1000).max(1);
                return AcquireResult::Denied {
                    retry_after_seconds,
                };
            }
            let next = current - 1000;
            if self
                .available_mtokens
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return AcquireResult::Allowed;
            }
        }
    }

    fn try_refill(&self, now: Instant) {
        let now_ms = self.now_ms(now);
        let last_ms = self.last_refill_ms.load(Ordering::Relaxed);
        let elapsed_ms = now_ms.saturating_sub(last_ms);
        if elapsed_ms == 0 {
            return;
        }

        // Only one thread performs refill; others continue without blocking.
        let refill_guard = match self.refill_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let _keep_guard = refill_guard;

        let refreshed_last = self.last_refill_ms.load(Ordering::Relaxed);
        let refreshed_elapsed_ms = now_ms.saturating_sub(refreshed_last);
        if refreshed_elapsed_ms == 0 {
            return;
        }

        // rate_per_second mtokens are added per ms
        let refill_mtokens = refreshed_elapsed_ms.saturating_mul(self.rate_per_second);
        let current = self.available_mtokens.load(Ordering::Relaxed);
        let next = current
            .saturating_add(refill_mtokens)
            .min(self.capacity_mtokens);
        self.available_mtokens.store(next, Ordering::Relaxed);
        self.last_refill_ms.store(now_ms, Ordering::Relaxed);
    }
}

pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS || request.uri().path() == "/health" {
        return next.run(request).await;
    }

    if let AcquireResult::Denied {
        retry_after_seconds,
    } = state.try_acquire(Instant::now())
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", retry_after_seconds.to_string())],
            Json(serde_json::json!({
                "error": {
                    "message": "Rate limit exceeded",
                    "type": "rate_limit_error",
                    "code": "429"
                }
            })),
        )
            .into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_token_bucket_refill_and_consume() {
        let state = RateLimitState::new(2);
        let now = Instant::now();

        assert!(matches!(state.try_acquire(now), AcquireResult::Allowed));
        assert!(matches!(state.try_acquire(now), AcquireResult::Allowed));
        assert!(matches!(
            state.try_acquire(now),
            AcquireResult::Denied { .. }
        ));

        // 500ms with 2rps refills 1 token
        assert!(matches!(
            state.try_acquire(now + Duration::from_millis(500)),
            AcquireResult::Allowed
        ));
        assert!(matches!(
            state.try_acquire(now + Duration::from_millis(500)),
            AcquireResult::Denied { .. }
        ));
    }

    #[test]
    fn test_retry_after_is_dynamic() {
        let state = RateLimitState::new(2);
        let now = Instant::now();
        let _ = state.try_acquire(now);
        let _ = state.try_acquire(now);

        match state.try_acquire(now) {
            AcquireResult::Denied {
                retry_after_seconds,
            } => assert_eq!(retry_after_seconds, 1),
            AcquireResult::Allowed => panic!("expected denied"),
        }
    }
}
